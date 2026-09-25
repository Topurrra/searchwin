//! Clipboard history.
//!
//! Captures every text copy event into a bounded ring buffer (last 200 by
//! default) so the user can recall, pin, and re-paste past clipboard
//! contents. Runs entirely locally — no network, no cloud sync, no tracking.
//!
//! **Capture model — event-driven, zero idle CPU.** A background thread
//! creates a hidden message-only window and registers it with
//! `AddClipboardFormatListener`. Windows then delivers a `WM_CLIPBOARDUPDATE`
//! message every time the clipboard changes. The thread blocks in a Win32
//! message pump (`GetMessageW`) when idle — actual CPU usage at rest is 0%.
//! When a message arrives, we open the clipboard once, read UTF-16 text,
//! identify the source application, run the sensitive-content scanner to
//! *tag* (not skip) credentials, and auto-categorize the content (URL,
//! email, code, JSON, color, hash, file path) for the UI.
//!
//! Sensitive content is **not skipped outright** — if you copied an API key
//! you almost certainly want to paste it. But the scanner *tags* it, and a
//! tagged entry expires on a deliberately short retention window (a few
//! minutes — `SENSITIVE_RETENTION_MS`) instead of the normal one, so
//! credentials don't linger for days. We also honour the OS "exclude from
//! clipboard monitors" marker that password fields set, and a user-editable
//! app exclusion list — capturing from KeePassXC / 1Password is off by
//! default since those apps already mark their clipboard data as transient.
//!
//! **Persistence — debounced.** Mutations schedule a save 500ms later and
//! coalesce — rapid copies don't produce write amplification. The data lives
//! in a single file under the app data preferences/ directory, DPAPI-
//! encrypted at rest on Windows.
//!
//! **Frontend updates — event-driven.** Every successful capture or mutation
//! emits a `clipboard-history-updated` Tauri event so the UI can refresh
//! without polling.

use crate::commands::sensitive_scan;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

/// The Win32 clipboard format id for "Unicode (UTF-16LE) text". Defined as
/// raw `13_u32` rather than reaching into `windows::Win32::System::...` for
/// the named constant because the path of `CF_UNICODETEXT` keeps shifting
/// between windows-rs versions; the value is stable since Windows 95.
#[cfg(windows)]
const CF_UNICODETEXT: u32 = 13;

/// Device-independent bitmap (BITMAPINFOHEADER + pixel data) — the clipboard
/// format produced when you copy an image from Snipping Tool, Photoshop,
/// Word, browsers, etc. Stable since Windows 3.1.
#[cfg(windows)]
const CF_DIB: u32 = 8;

/// File-drop list — what File Explorer puts on the clipboard when you copy
/// (not cut) one or more files. The data is a DROPFILES struct followed by
/// null-terminated file paths. We use this as a workaround for browsers
/// that re-encode GIFs to PNG on "Copy image": the user can right-click →
/// Save image, then copy the saved .gif file from Explorer, and we capture
/// the actual animated bytes.
#[cfg(windows)]
const CF_HDROP: u32 = 15;

/// MIME-typed registered clipboard formats. Browsers, Discord, Slack and
/// modern image tools put the original file bytes onto the clipboard under
/// these names alongside CF_DIB — reading the registered format lets us
/// preserve animation (GIF, animated WebP) and avoid re-encoding losses
/// (PNG, JPEG). IDs are runtime-allocated by RegisterClipboardFormatW; we
/// cache them in OnceLocks so the lookup happens at most once per format.
#[cfg(windows)]
static FORMAT_GIF: OnceLock<u32> = OnceLock::new();
#[cfg(windows)]
static FORMAT_PNG: OnceLock<u32> = OnceLock::new();
#[cfg(windows)]
static FORMAT_JPEG: OnceLock<u32> = OnceLock::new();
#[cfg(windows)]
static FORMAT_WEBP: OnceLock<u32> = OnceLock::new();

/// Registered format that password fields / secure inputs place on the
/// clipboard to ask monitors (clipboard managers, Windows clipboard history)
/// not to store the content. We honour it — see `clipboard_marked_no_capture`.
#[cfg(windows)]
static FORMAT_EXCLUDE_MONITOR: OnceLock<u32> = OnceLock::new();

/// `CanIncludeInClipboardHistory`: a DWORD Windows' own history (Win+V)
/// honours, 0 meaning "don't keep this". Password managers and Search's
/// QuietCopy set it; the history here honours it too.
#[cfg(windows)]
static FORMAT_HISTORY_MARKER: OnceLock<u32> = OnceLock::new();

/// Hard ceiling on retained entries. Older entries fall off the back when this
/// is exceeded. Pinned entries are never auto-evicted regardless of position.
const MAX_ENTRIES: usize = 200;

/// Default retention for non-pinned entries — items older than this get
/// purged on the next cleanup tick. Users can override per-install via the
/// `set_clipboard_retention_days(n)` command; `0` disables time-based
/// expiry (only the MAX_ENTRIES cap then applies). Pinned entries are
/// never time-expired regardless of this setting.
const DEFAULT_RETENTION_DAYS: u32 = 14;

/// How often the save thread runs the retention sweep. The sweep itself is
/// cheap (one ring-scan) so we can do it generously; this caps the staleness
/// of an expired entry that's still visible in UI between captures.
const RETENTION_SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// Retention window for entries the scanner flagged as sensitive (passwords,
/// tokens, card numbers). Deliberately short — long enough to paste a copied
/// secret, short enough that credentials don't linger in history for days.
/// Applies regardless of the general retention setting; pinned entries are
/// exempt (the user explicitly chose to keep them).
const SENSITIVE_RETENTION_MS: i64 = 5 * 60 * 1000;

/// Trim very long clipboard text to a reasonable preview — full text is still
/// stored, this is just a safety net against pathological multi-megabyte copies
/// from e.g. accidentally copying a whole file's contents.
const MAX_ENTRY_BYTES: usize = 256 * 1024; // 256 KB

/// How long we wait after a mutation before actually writing the JSON file.
/// Coalesces rapid changes (e.g. paste-loop workflows) into one disk write.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

/// Tauri event name. Emitted whenever the history changes for any reason —
/// new capture, pin/unpin, delete, clear, exclusion edit, pause/resume.
const HISTORY_UPDATED_EVENT: &str = "clipboard-history-updated";

/// Tauri event name. Emitted when an auto-paste attempt couldn't follow
/// through (SetForegroundWindow refused, SendInput partially failed, etc.).
/// The payload is a short user-facing message the frontend surfaces as a
/// toast. Text is always already on the clipboard at this point — the
/// fallback is "press Ctrl+V manually".
const PASTE_FALLBACK_EVENT: &str = "clipboard-paste-fallback";

const PREFERENCES_SUBDIR: &str = "preferences";
const HISTORY_FILE: &str = "clipboard_history.json";
const EXCLUSIONS_FILE: &str = "clipboard_exclusions.json";
/// Subdirectory under app data where captured images live as PNG files.
/// One file per entry, named `<id>.png`. Deletion of an entry removes its
/// file too — the original on whatever app you copied from is never touched.
const IMAGE_STORAGE_SUBDIR: &str = "clipboard-images";
/// Hard ceiling on a single captured image's encoded size. Bigger than this
/// is almost certainly a wallpaper-grab or screenshot of a 4K monitor; we
/// log and skip so a single huge copy can't blow up the history's disk
/// footprint. ~12 MB is roughly an uncompressed 4K screenshot's worth of
/// pixels after PNG encoding for typical content.
const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;

/// Cumulative disk budget for the captured-image cache. Once the total
/// of all image-entry sizes exceeds this, oldest non-pinned images are
/// evicted (their entries removed AND their files unlinked) until we're
/// back under budget. Pinned images are immune — the user explicitly
/// preserved them, so we keep them even if doing so blows the budget.
///
/// 256 MB is enough for ~50-150 typical screenshots while staying well
/// inside what an antivirus or backup tool would consider "normal cache".
/// Without this, a heavy image-clipboard workflow can silently push the
/// app data dir into multi-GB territory over a few weeks.
const MAX_IMAGE_DISK_BYTES: u64 = 256 * 1024 * 1024;

/// Bounding box (px) for the list-view thumbnail generated at capture time.
/// 320 px stays crisp on HiDPI displays while encoding to a few tens of KB —
/// small enough that a 200-entry history of screenshots renders the list
/// without decoding hundreds of MB of full images.
const THUMBNAIL_MAX_DIM: u32 = 320;

/// Default exclusion list — common password managers and secret stores. We
/// match case-insensitively against the foreground process name (without the
/// `.exe` suffix). Users can edit this list in the UI; their changes override
/// the defaults.
///
/// Rationale: these apps put the password on the clipboard for ~20-30 seconds
/// then clear it. Capturing into a permanent history defeats the security
/// model they explicitly opt into. Excluding by default is the right
/// behavior; users who *want* clipboard history from their password manager
/// can remove the entry.
const DEFAULT_EXCLUSIONS: &[&str] = &[
    "KeePassXC",
    "KeePass",
    "1Password",
    "Bitwarden",
    "Dashlane",
    "ProtonPass",
    "LastPass",
    "Enpass",
    "RoboForm",
    "NordPass",
    "Keeper",
];

/// Detected content type. We do best-effort classification at capture time
/// so the UI can show category icons and offer quick filters in Phase 2.
/// Order matters: higher-precision categories come first in `categorize()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Plain text with no recognizable structure.
    Text,
    /// http://, https://, ftp://, file://
    Url,
    /// foo@bar.com
    Email,
    /// Looks like a Windows or POSIX file path.
    FilePath,
    /// Wraps in `{}` or `[]` and parses as JSON.
    Json,
    /// CSS color (#rgb, #rrggbb, rgb(), rgba(), hsl()) or named common color.
    Color,
    /// 32+ hex chars (MD5 / SHA-1 / SHA-256 / UUIDs / hashes).
    Hash,
    /// Multi-line content with code-shaped indicators (braces, semicolons,
    /// `function`/`def`/`fn`/`class` keywords, etc.).
    Code,
    /// A single number — useful for "I just copied the OTP" cases.
    Number,
    /// Image — only used for entries with `kind: Image`. Sensitive scanning
    /// + textual category detection skipped for images per design.
    Image,
}

/// Discriminant for what kind of clipboard data this entry holds. The
/// payload fields (text / image_path) are populated based on this. Stored
/// in serialized form as `"text"` / `"image"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Text,
    Image,
}

fn default_entry_kind() -> EntryKind {
    EntryKind::Text
}

/// One captured clipboard event. Serializable so we can round-trip via Tauri
/// commands and the JSON file on disk. Text and image entries share this
/// struct; the `kind` discriminant + presence of `image_*` fields tells you
/// which one. The textual fields (`text`, `sensitive_kinds`) are empty for
/// image entries and vice versa.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardEntry {
    pub id: u64,
    /// Unix ms timestamp of when we captured this entry.
    pub captured_at_ms: i64,
    /// Text vs Image discriminant. New in image-support release; old
    /// persisted entries that lack this field default to Text via serde.
    #[serde(default = "default_entry_kind")]
    pub kind: EntryKind,
    /// Full clipboard text (trimmed to MAX_ENTRY_BYTES at capture time).
    /// Empty for image entries.
    #[serde(default)]
    pub text: String,
    /// Best-effort process name of the app whose window was focused at the
    /// moment of copy. `None` if Win32 lookup failed (rare; usually means an
    /// elevated process copied while KeepItLocal runs non-elevated).
    pub source_app: Option<String>,
    /// Best-effort full path to the source app's executable, for resolving its
    /// icon in the UI. `None` for entries captured before this field existed,
    /// or when the path lookup failed.
    #[serde(default)]
    pub source_app_path: Option<String>,
    /// Tags from the sensitive scanner — empty for image entries and for
    /// normal text. Populated with things like "github_token", "credit_card"
    /// when the content looks like a credential or PII.
    #[serde(default)]
    pub sensitive_kinds: Vec<String>,
    /// Auto-detected content category. Drives UI icons + future quick filters.
    /// Image entries always carry Category::Image.
    #[serde(default = "default_category")]
    pub category: Category,
    /// Sticky entries the user has explicitly kept. Never auto-evicted.
    pub is_pinned: bool,
    /// Optional user-set label for pinned items ("my email signature").
    /// Always None for non-pinned entries.
    #[serde(default)]
    pub pin_label: Option<String>,
    /// Absolute path to the PNG copy on disk. Only set for image entries.
    /// Deleting the entry removes this file; the original you copied from
    /// is never touched.
    #[serde(default)]
    pub image_path: Option<PathBuf>,
    /// Absolute path to a small PNG thumbnail generated at capture time.
    /// The list views and overlay render this instead of decoding the full
    /// image, so a history full of multi-MB screenshots stays light. `None`
    /// for text entries, for image entries persisted before thumbnail support
    /// landed (the UI falls back to `image_path`), or if generation failed.
    #[serde(default)]
    pub thumbnail_path: Option<PathBuf>,
    /// Image dimensions in pixels. Useful for the UI to size thumbnails
    /// without a round-trip through the file.
    #[serde(default)]
    pub image_width: Option<u32>,
    #[serde(default)]
    pub image_height: Option<u32>,
    /// Size of the on-disk image in bytes. Lets the UI show "1.2 MB" without
    /// stat'ing the file every render.
    #[serde(default)]
    pub image_size_bytes: Option<u64>,
    /// Original image format the source app placed on the clipboard.
    /// Recognized values: "gif" (animation preserved), "png", "jpeg", "webp",
    /// "bmp" (from CF_DIB fallback). The file at `image_path` is bytes-
    /// identical to whatever the source app held, except for the CF_DIB
    /// case where we transcoded to PNG for storage. Used at paste-back time
    /// to write the matching registered format back, so a copied GIF stays
    /// animated when pasted into Discord / browsers / etc.
    #[serde(default)]
    pub image_format: Option<String>,
}

fn default_category() -> Category {
    Category::Text
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionsState {
    /// Process names (without `.exe`) to skip capturing from. Case-insensitive
    /// match against the foreground app at copy time.
    pub apps: Vec<String>,
}

struct State {
    entries: VecDeque<ClipboardEntry>,
    next_id: u64,
    /// Process-name exclusion list. Loaded from disk on startup, mutable via
    /// commands. Stored lowercased for fast case-insensitive matching.
    exclusions: Vec<String>,
    /// User-toggled global pause. When true, the listener still fires but we
    /// drop captures on the floor without indexing or persisting.
    paused: bool,
    /// Set while we're writing our own value to the clipboard so the
    /// listener can skip the resulting WM_CLIPBOARDUPDATE — otherwise picking
    /// a past entry would re-capture it as a brand-new copy. A deadline (ms
    /// since 1970), not a flag: a write whose update never came must not
    /// swallow the user's next real copy minutes later.
    suppress_until_ms: i64,
    /// Max age of non-pinned entries before they're swept by the cleanup
    /// pass. `0` disables time-based expiry (the MAX_ENTRIES cap still
    /// applies). Persisted alongside the entries file.
    retention_days: u32,
    /// Whether image / GIF clipboard data is captured. **Default on**
    /// (changed 2026-05-26 per user verdict — previously off, "opt-in",
    /// which silently dropped half of what users copy). Image entries
    /// are bounded by `image_retention_days` so the disk footprint stays
    /// reasonable. Off-state ignores image copies entirely; the
    /// original on the OS clipboard is unaffected.
    images_enabled: bool,
    /// One-time migration sentinel for the 2026-05-26 default flip from
    /// `false` → `true`. `load_from_disk` checks this on every load; if
    /// `false` (pre-migration install), it forces `images_enabled = true`
    /// and stamps this flag, then schedules a save so the change
    /// persists. Subsequent loads see the flag and leave the user's
    /// chosen value alone — so a user who turns the toggle off *after*
    /// the migration keeps that choice on every load thereafter.
    images_default_v2_applied: bool,
    /// Separate retention window for image entries — kept shorter than text
    /// retention by default because each file is many MB. 0 disables image
    /// time-expiry.
    image_retention_days: u32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            next_id: 0,
            // Lowercased copies of DEFAULT_EXCLUSIONS — overwritten on
            // load_exclusions_from_disk() if the user has saved custom list.
            exclusions: DEFAULT_EXCLUSIONS
                .iter()
                .map(|s| s.to_lowercase())
                .collect(),
            paused: false,
            suppress_until_ms: 0,
            retention_days: DEFAULT_RETENTION_DAYS,
            // Fresh installs: images on, migration already applied (the
            // migration only exists for installs that predate the flip).
            images_enabled: true,
            images_default_v2_applied: true,
            image_retention_days: 2,
        }
    }
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static LISTENER_STARTED: AtomicBool = AtomicBool::new(false);
/// Stashed AppHandle so the Win32 WindowProc (which has a C signature and
/// can't take captured state via closure) can reach Tauri APIs for emitting
/// events and resolving the app data directory.
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
/// Unix-ms timestamp of the most recent mutation, set by `schedule_save()`.
/// The debouncer thread polls this to decide when to actually flush.
static SAVE_DEADLINE: AtomicI64 = AtomicI64::new(0);

/// HWND (as isize, since HWND wraps that natively) of the window the user
/// was working in *before* they invoked the clipboard overlay. Captured by
/// `remember_foreground_window()` just before the overlay takes focus, and
/// used by `paste_clipboard_entry` to refocus that window and inject a
/// synthetic Ctrl+V. A value of 0 means "no captured target" — pastes will
/// fall back to copy-only behavior.
static PREVIOUS_FOREGROUND_HWND: AtomicIsize = AtomicIsize::new(0);

/// Holds a pending corrupt-history recovery notice — the quarantine filename
/// — set by `load_from_disk` when it had to set aside an unreadable history
/// file. Parked here rather than emitted as an event because `load_from_disk`
/// runs at startup when no window is open to receive it; the in-app Clipboard
/// page pulls it on mount via `take_clipboard_recovery_notice`.
static RECOVERY_NOTICE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

/// BLAKE3 of the last captured image + when we saw it, for burst suppression.
/// Snipping Tool (and any app that fills the clipboard across more than one
/// Open/Close cycle) fires several `WM_CLIPBOARDUPDATE` for ONE user copy.
/// Text survives that because `push_text_entry` dedups against the whole ring;
/// images had no dedup at all (deliberately — see `push_image_entry`), so a
/// single snip landed as two identical entries.
///
/// ponytail: content hash + short window, NOT full-ring image dedup. This
/// only collapses the burst from one physical copy; deliberately re-copying
/// the same image later still gets its own entry, which preserves the
/// documented v1 "every image copy is its own entry" decision. If we ever do
/// want true image dedup, the upgrade path is a hash field on ClipboardEntry
/// (schema bump + migration) rather than widening this window.
static LAST_IMAGE: OnceLock<Mutex<Option<([u8; 32], i64)>>> = OnceLock::new();

/// How close together two identical images must land to count as one copy.
/// The real bursts are milliseconds apart; 2s is slack for a slow machine
/// without being long enough to swallow an intentional re-copy.
const IMAGE_BURST_WINDOW_MS: i64 = 2_000;

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

fn recovery_notice() -> &'static Mutex<Option<String>> {
    RECOVERY_NOTICE.get_or_init(|| Mutex::new(None))
}

fn last_image() -> &'static Mutex<Option<([u8; 32], i64)>> {
    LAST_IMAGE.get_or_init(|| Mutex::new(None))
}

/// True when `bytes` is the same image we just captured moments ago — i.e. a
/// repeat `WM_CLIPBOARDUPDATE` for one user copy, not a second copy. Records
/// the hash either way so the next event can compare against it. Recovers from
/// poisoning for the same reason `locked_state` does: one panic elsewhere must
/// not permanently break clipboard capture.
fn is_duplicate_image_burst(bytes: &[u8]) -> bool {
    let digest = *blake3::hash(bytes).as_bytes();
    let now = now_ms();
    let mut guard = last_image()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let dup = matches!(*guard, Some((prev, ts)) if prev == digest && now - ts < IMAGE_BURST_WINDOW_MS);
    *guard = Some((digest, now));
    dup
}

/// Acquire the global state mutex, recovering from poisoning instead of
/// panicking. The State struct holds clipboard entries, exclusions, and
/// settings flags — none of those fields become internally inconsistent
/// if a previous lock-holder panicked, so taking the data via
/// `into_inner()` keeps the daemon alive across transient panics in
/// unrelated handlers. Using a panicking `.unwrap()` here would mean
/// one rare panic permanently breaks every clipboard command for the
/// rest of the session.
fn locked_state() -> std::sync::MutexGuard<'static, State> {
    state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Largest index <= `max` that is a UTF-8 char boundary in `s`.
///
/// `String::truncate` PANICS if the index splits a multi-byte character, and
/// the capture path that truncates runs inside the Win32 window proc — an
/// `extern "system"` fn, where unwinding past the FFI boundary is UB. So a
/// single 256KB+ copy containing Georgian, Cyrillic, CJK or an emoji near the
/// cut point could take the process down. ASCII text never trips it, which is
/// why this survived.
///
/// (std has `floor_char_boundary`, still unstable as of this toolchain.)
/// Loops at most 3 times: UTF-8 characters are 4 bytes max.
fn floor_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn emit_history_updated() {
    if let Some(app) = APP_HANDLE.get() {
        let _ = app.emit(HISTORY_UPDATED_EVENT, ());
    }
}

/// Surface a "paste didn't go through" hint to whichever window is currently
/// showing. Always-safe to call — broadcasts globally and any listening
/// window pops a toast. If no window is open, the event is dropped (and
/// that's fine: the user already has the text on their clipboard).
fn emit_paste_fallback(message: &str) {
    if let Some(app) = APP_HANDLE.get() {
        let _ = app.emit(PASTE_FALLBACK_EVENT, message.to_string());
    }
}

// ─── Auto-categorization ─────────────────────────────────────────────────

/// Best-effort content classification. Cheap, runs at capture time. Wrong
/// categorization is harmless — the UI shows it as a hint, not a constraint.
fn categorize(text: &str) -> Category {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Category::Text;
    }

    // URL — explicit scheme is the most reliable signal. We accept anything
    // that starts with a known scheme; no need to validate the full URL.
    let lower = trimmed.to_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ftp://")
        || lower.starts_with("file://")
        || lower.starts_with("ssh://")
    {
        return Category::Url;
    }

    // Single-line single-token email — strict-ish: must have @ and a TLD.
    if !trimmed.contains(char::is_whitespace) && trimmed.contains('@') {
        if looks_like_email(trimmed) {
            return Category::Email;
        }
    }

    // JSON — wraps the whole content in {} or []. Doesn't fully parse but
    // checks structural balance to avoid false positives on `{not json}`.
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        if looks_like_json(trimmed) {
            return Category::Json;
        }
    }

    // CSS color — hex shorthand/full, rgb/rgba/hsl/hsla functions.
    if looks_like_color(trimmed) {
        return Category::Color;
    }

    // Windows/POSIX path. Heuristic: contains a path separator and looks like
    // a path "shape" (drive letter on Windows, leading slash on POSIX, or
    // multiple separators in a single token).
    if looks_like_file_path(trimmed) {
        return Category::FilePath;
    }

    // Hash / hex blob — 32+ hex chars in one run. Catches MD5 (32), SHA-1
    // (40), SHA-256 (64), UUIDs (with/without dashes), git SHAs.
    if looks_like_hash(trimmed) {
        return Category::Hash;
    }

    // Plain number — common for OTPs, ids, totals. Stricter than "starts
    // with digit" — must be ONLY digits with maybe a sign and decimal.
    if looks_like_number(trimmed) {
        return Category::Number;
    }

    // Multi-line + brace/keyword shape → probably code.
    if looks_like_code(trimmed) {
        return Category::Code;
    }

    Category::Text
}

fn looks_like_email(s: &str) -> bool {
    let parts: Vec<&str> = s.split('@').collect();
    if parts.len() != 2 {
        return false;
    }
    let (local, domain) = (parts[0], parts[1]);
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    // Domain must have a dot and an alphabetic TLD of 2+ chars.
    let tld = match domain.rsplit('.').next() {
        Some(t) if t.len() >= 2 && t.chars().all(|c| c.is_ascii_alphabetic()) => t,
        _ => return false,
    };
    let _ = tld;
    domain.contains('.')
}

fn looks_like_json(s: &str) -> bool {
    // We don't need to fully parse — just confirm braces balance AND that the
    // content has JSON-shaped punctuation (a `:` or `,` outside strings, OR
    // it's the empty `{}` / `[]` case). Without the punctuation check, prose
    // wrapped in curlies like "{just a sentence}" would false-positive.
    let mut depth_curly: i32 = 0;
    let mut depth_square: i32 = 0;
    let mut in_string = false;
    let mut prev_backslash = false;
    let mut saw_json_punct = false;
    let mut saw_quoted_string = false;
    for c in s.chars() {
        if in_string {
            if prev_backslash {
                prev_backslash = false;
                continue;
            }
            if c == '\\' {
                prev_backslash = true;
                continue;
            }
            if c == '"' {
                in_string = false;
                saw_quoted_string = true;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth_curly += 1,
            '}' => depth_curly -= 1,
            '[' => depth_square += 1,
            ']' => depth_square -= 1,
            ':' | ',' => saw_json_punct = true,
            _ => {}
        }
        if depth_curly < 0 || depth_square < 0 {
            return false;
        }
    }
    if depth_curly != 0 || depth_square != 0 {
        return false;
    }
    // Empty arrays/objects are valid JSON even without punctuation.
    let trimmed = s.trim();
    if trimmed == "{}" || trimmed == "[]" {
        return true;
    }
    saw_json_punct || saw_quoted_string
}

fn looks_like_color(s: &str) -> bool {
    let lower = s.to_lowercase();
    // Hex: #fff, #ffff, #ffffff, #ffffffff
    if let Some(rest) = lower.strip_prefix('#') {
        if matches!(rest.len(), 3 | 4 | 6 | 8) && rest.chars().all(|c| c.is_ascii_hexdigit()) {
            return true;
        }
    }
    // Functional notation
    for prefix in &["rgb(", "rgba(", "hsl(", "hsla("] {
        if lower.starts_with(prefix) && lower.ends_with(')') {
            return true;
        }
    }
    false
}

fn looks_like_file_path(s: &str) -> bool {
    // Disqualify obvious non-paths: contains newlines or trims to <2 chars.
    if s.contains('\n') || s.len() < 2 {
        return false;
    }
    let bytes = s.as_bytes();
    // Windows drive letter: C:\ or D:/
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    // UNC: \\server\share
    if s.starts_with("\\\\") {
        return true;
    }
    // POSIX absolute: /usr/... or /home/...
    if s.starts_with('/') && s.len() > 2 && s.matches('/').count() >= 2 && !s.contains(' ') {
        return true;
    }
    // Relative path with backslash separators and no spaces: typical Windows.
    if s.contains('\\') && !s.contains(' ') && s.matches('\\').count() >= 1 {
        return true;
    }
    false
}

fn looks_like_hash(s: &str) -> bool {
    // Strip common hash-y separators (UUIDs have dashes).
    let stripped: String = s.chars().filter(|c| *c != '-').collect();
    if stripped.len() < 32 {
        return false;
    }
    if stripped.len() > 128 {
        return false;
    }
    stripped.chars().all(|c| c.is_ascii_hexdigit())
}

fn looks_like_number(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars().peekable();
    if matches!(chars.peek(), Some('-') | Some('+')) {
        chars.next();
    }
    let mut saw_digit = false;
    let mut saw_dot = false;
    for c in chars {
        if c.is_ascii_digit() {
            saw_digit = true;
        } else if c == '.' && !saw_dot {
            saw_dot = true;
        } else {
            return false;
        }
    }
    saw_digit
}

fn looks_like_code(s: &str) -> bool {
    // Cheap heuristic — multi-line AND contains code-shape indicators.
    if !s.contains('\n') {
        return false;
    }
    let signals: &[&str] = &[
        "{", "}", "();", "();\n", "fn ", "def ", "function ", "class ", "import ",
        "const ", "let ", "var ", "return ", "if (", "if(", "for (", "for(", "//", "/*",
        "#include", "package ", "public ", "private ",
    ];
    signals.iter().filter(|sig| s.contains(*sig)).count() >= 1
}

// ─── Persistence ─────────────────────────────────────────────────────────

fn preferences_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir lookup failed: {e}"))?
        .join(PREFERENCES_SUBDIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create preferences dir: {e}"))?;
    Ok(dir)
}

fn history_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(preferences_dir(app)?.join(HISTORY_FILE))
}

fn exclusions_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(preferences_dir(app)?.join(EXCLUSIONS_FILE))
}

/// Directory where captured image copies are stored. Created on demand.
/// Distinct from preferences/ so the user (or an "Erase clipboard images"
/// command in the future) can wipe just images without losing config.
fn image_storage_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir lookup failed: {e}"))?
        .join(IMAGE_STORAGE_SUBDIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create image dir: {e}"))?;
    Ok(dir)
}

/// Remove the on-disk files for an image entry — both the full PNG and its
/// thumbnail. Text entries are a no-op. We swallow errors (already-deleted
/// file, locked, etc.) because the entry is being evicted regardless —
/// leaving an orphan file is recoverable, blocking the eviction on a stuck
/// file is not.
fn delete_image_file_for_entry(entry: &ClipboardEntry) {
    if !matches!(entry.kind, EntryKind::Image) {
        return;
    }
    for path in [entry.image_path.as_ref(), entry.thumbnail_path.as_ref()]
        .into_iter()
        .flatten()
    {
        if let Err(error) = std::fs::remove_file(path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "clipboard_history: could not delete image file {:?}: {error}",
                    path
                );
            }
        }
    }
}

/// Schedule a debounced disk write. Multiple calls within the debounce
/// window collapse into a single write — important for rapid-copy workflows
/// that would otherwise hammer the filesystem.
fn schedule_save() {
    let deadline = now_ms() + SAVE_DEBOUNCE.as_millis() as i64;
    SAVE_DEADLINE.store(deadline, Ordering::SeqCst);
}

/// Background flush thread. Wakes periodically; flushes if the deadline has
/// passed since the last mutation. Cost: one Mutex lock + atomic check every
/// 200ms when nothing's happening (negligible). Also runs the retention
/// sweep once per RETENTION_SWEEP_INTERVAL to purge stale non-pinned entries.
fn run_save_thread(app: AppHandle) {
    let mut last_sweep_at_ms = now_ms();
    loop {
        std::thread::sleep(Duration::from_millis(200));

        // Time-based retention sweep — independent of any save deadline so
        // a mostly-idle app still trims old entries on schedule.
        if (now_ms() - last_sweep_at_ms) as u128
            >= RETENTION_SWEEP_INTERVAL.as_millis()
        {
            last_sweep_at_ms = now_ms();
            if run_retention_sweep() {
                // Sweep removed something → flush soon. Don't block here.
                schedule_save();
                emit_history_updated();
            }
        }

        let deadline = SAVE_DEADLINE.load(Ordering::SeqCst);
        if deadline == 0 {
            continue;
        }
        if now_ms() < deadline {
            continue;
        }
        // Race: another mutation may bump the deadline between our load and
        // the swap. CompareExchange ensures we only consume *this* deadline;
        // a fresh mutation re-sets it and we'll handle that on the next tick.
        if SAVE_DEADLINE
            .compare_exchange(deadline, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            continue;
        }
        flush_to_disk(&app);
    }
}

/// Pure retention predicate — `true` if `e` should be KEPT by a retention
/// sweep, given the three cutoffs. Pinned entries always survive; a
/// scanner-flagged sensitive entry expires on the short `sensitive_cutoff`;
/// otherwise a text / image entry expires once older than its own cutoff
/// (`None` = time-based retention disabled for that kind). Extracted from
/// `run_retention_sweep` so the branching is unit-testable without the global
/// state singleton.
fn entry_survives_retention(
    e: &ClipboardEntry,
    text_cutoff: Option<i64>,
    image_cutoff: Option<i64>,
    sensitive_cutoff: i64,
) -> bool {
    if e.is_pinned {
        return true;
    }
    if !e.sensitive_kinds.is_empty() && e.captured_at_ms < sensitive_cutoff {
        return false;
    }
    let cutoff_opt = match e.kind {
        EntryKind::Text => text_cutoff,
        EntryKind::Image => image_cutoff,
    };
    !matches!(cutoff_opt, Some(cutoff) if e.captured_at_ms < cutoff)
}

/// Time-expire non-pinned entries older than the configured retention.
/// Text and image entries use independent retention windows because images
/// are storage-heavy and most users want them rotated out faster than text.
/// Image entries also have their on-disk PNG unlinked so disk usage drops
/// proportionally. Pinned entries (text or image) are immune.
fn run_retention_sweep() -> bool {
    let removed: Vec<ClipboardEntry> = {
        let mut guard = locked_state();
        let text_retention = guard.retention_days;
        let image_retention = guard.image_retention_days;

        let now = now_ms();
        let text_cutoff = if text_retention > 0 {
            Some(now - (text_retention as i64) * 24 * 60 * 60 * 1000)
        } else {
            None
        };
        let image_cutoff = if image_retention > 0 {
            Some(now - (image_retention as i64) * 24 * 60 * 60 * 1000)
        } else {
            None
        };
        // Entries the scanner flagged as sensitive expire on a short window
        // regardless of the general retention setting — long enough to paste
        // a copied secret, short enough not to hoard credentials.
        let sensitive_cutoff = now - SENSITIVE_RETENTION_MS;

        let mut removed = Vec::new();
        guard.entries.retain(|e| {
            if entry_survives_retention(e, text_cutoff, image_cutoff, sensitive_cutoff) {
                true
            } else {
                removed.push(e.clone());
                false
            }
        });
        removed
    };

    // File cleanup happens outside the lock — disk I/O shouldn't block state
    // access from the listener thread.
    for entry in &removed {
        delete_image_file_for_entry(entry);
    }

    // Run the disk-budget enforcer too. Even with retention disabled, this
    // converges any existing oversized install back to the budget over time
    // — without it, a user who upgrades to this version with 2 GB of cached
    // images keeps that 2 GB forever as long as they pin nothing.
    let budget_evicted = {
        let mut guard = locked_state();
        let before = guard.entries.len();
        evict_to_image_budget(&mut guard);
        before != guard.entries.len()
    };

    !removed.is_empty() || budget_evicted
}

/// On-disk schema for the persisted clipboard state. Wrapped in a struct
/// (vs. a bare entries array) so we can add config fields like retention
/// without changing the file format incompatibly. Reads tolerate both the
/// new struct form AND the old bare-array form via `load_from_disk`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryFile {
    #[serde(default)]
    entries: Vec<ClipboardEntry>,
    #[serde(default = "default_retention_days_serde")]
    retention_days: u32,
    #[serde(default)]
    images_enabled: bool,
    /// Pre-2026-05-26 files don't have this field; serde-default `false`
    /// → `load_from_disk` runs the one-time flip-to-true migration. New
    /// files always have it `true` (set on first save after migration
    /// runs, or by `State::default()` for fresh installs).
    #[serde(default)]
    images_default_v2_applied: bool,
    #[serde(default = "default_image_retention_days_serde")]
    image_retention_days: u32,
}

fn default_retention_days_serde() -> u32 {
    DEFAULT_RETENTION_DAYS
}

fn default_image_retention_days_serde() -> u32 {
    2
}

/// At-rest encryption for the clipboard history file. On Windows the bytes
/// are DPAPI-protected — the same per-user envelope `local_db` uses, so a copy
/// of the file is opaque to another account / machine. Other platforms pass
/// through unchanged until a platform keystore is wired up.
///
/// Search: a failed encrypt is None, and the save is skipped — what you
/// copied never lands on disk in the clear. The history stays in memory and
/// the next save tries again.
#[cfg(windows)]
fn protect_at_rest(plaintext: &[u8]) -> Option<Vec<u8>> {
    seal_or_skip(plaintext, crate::core::dpapi::protect)
}

#[cfg(not(windows))]
fn protect_at_rest(plaintext: &[u8]) -> Option<Vec<u8>> {
    Some(plaintext.to_vec())
}

/// The sealed bytes, or None (logged) when sealing failed — never the
/// plaintext in their place.
fn seal_or_skip(plaintext: &[u8], seal: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>) -> Option<Vec<u8>> {
    match seal(plaintext) {
        Ok(sealed) => Some(sealed),
        Err(error) => {
            eprintln!("clipboard_history: at-rest encrypt failed ({error}); not saving");
            None
        }
    }
}

/// Inverse of `protect_at_rest`. `dpapi::unprotect` already passes a legacy
/// plaintext file through unchanged (it checks a version byte), so an existing
/// unencrypted history file upgrades transparently — the next save re-writes
/// it encrypted. A genuine decrypt failure returns the blob unchanged so the
/// JSON parse fails and `load_from_disk`'s corrupt-file quarantine takes over.
#[cfg(windows)]
fn unprotect_at_rest(blob: &[u8]) -> Vec<u8> {
    crate::core::dpapi::unprotect(blob).unwrap_or_else(|_| blob.to_vec())
}

#[cfg(not(windows))]
fn unprotect_at_rest(blob: &[u8]) -> Vec<u8> {
    blob.to_vec()
}

fn flush_to_disk(app: &AppHandle) {
    let path = match history_path(app) {
        Ok(p) => p,
        Err(error) => {
            eprintln!("clipboard_history: cannot resolve path: {error}");
            return;
        }
    };
    let payload = {
        let guard = locked_state();
        HistoryFile {
            entries: guard.entries.iter().cloned().collect(),
            retention_days: guard.retention_days,
            images_enabled: guard.images_enabled,
            images_default_v2_applied: guard.images_default_v2_applied,
            image_retention_days: guard.image_retention_days,
        }
    };
    let json = match serde_json::to_vec_pretty(&payload) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("clipboard_history: serialize failed: {error}");
            return;
        }
    };
    let Some(encrypted) = protect_at_rest(&json) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if let Err(error) = std::fs::write(&tmp, &encrypted) {
        eprintln!("clipboard_history: write tmp failed: {error}");
        return;
    }
    if let Err(error) = std::fs::rename(&tmp, &path) {
        eprintln!("clipboard_history: atomic rename failed: {error}");
    }
}

fn save_exclusions(app: &AppHandle) {
    let path = match exclusions_path(app) {
        Ok(p) => p,
        Err(_) => return,
    };
    let exclusions = locked_state().exclusions.clone();
    let state_payload = ExclusionsState { apps: exclusions };
    let Ok(json) = serde_json::to_vec_pretty(&state_payload) else { return };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, &json).is_err() {
        return;
    }
    let _ = std::fs::rename(&tmp, &path);
}

fn load_from_disk(app: &AppHandle) {
    // History — accepts both the new wrapped struct AND the old bare-array
    // form for backwards compatibility with files written before retention
    // was added. Try the new form first; fall through to the legacy form on
    // parse failure.
    if let Ok(path) = history_path(app) {
        if let Ok(bytes) = std::fs::read(&path) {
            // Decrypt at-rest protection (a legacy plaintext file passes
            // through unchanged, so existing installs upgrade transparently).
            let bytes = unprotect_at_rest(&bytes);
            if let Ok(file) = serde_json::from_slice::<HistoryFile>(&bytes) {
                let mut guard = locked_state();
                let max_id = file.entries.iter().map(|e| e.id).max().unwrap_or(0);
                guard.entries = file.entries.into_iter().collect();
                guard.next_id = max_id + 1;
                guard.retention_days = file.retention_days;
                guard.images_enabled = file.images_enabled;
                guard.images_default_v2_applied = file.images_default_v2_applied;
                guard.image_retention_days = file.image_retention_days;

                // One-time migration (2026-05-26): if the persisted file
                // predates the images-default-on flip, force the flag on
                // and stamp the migration sentinel. The next save (via
                // `schedule_save` below) persists both. Subsequent loads
                // see `images_default_v2_applied == true` and leave the
                // user's chosen value untouched — so a user who turns the
                // toggle off *after* this migration keeps their off
                // preference forever.
                if !guard.images_default_v2_applied {
                    guard.images_enabled = true;
                    guard.images_default_v2_applied = true;
                    drop(guard);
                    schedule_save();
                }
            } else if let Ok(entries) =
                serde_json::from_slice::<Vec<ClipboardEntry>>(&bytes)
            {
                // Legacy bare-array file from Phase 1 — keep the entries,
                // leave retention at the default.
                let mut guard = locked_state();
                let max_id = entries.iter().map(|e| e.id).max().unwrap_or(0);
                guard.entries = entries.into_iter().collect();
                guard.next_id = max_id + 1;
            } else {
                // The file exists and is readable but parses as neither the
                // current nor the legacy format — it is corrupt. Quarantine it
                // (rename aside) rather than letting the next flush silently
                // overwrite it: the bytes are preserved for inspection / manual
                // recovery, the daemon does not crash, and clipboard history
                // simply starts empty.
                let quarantine =
                    path.with_file_name(format!("history.corrupt-{}.json", now_ms()));
                match std::fs::rename(&path, &quarantine) {
                    Ok(()) => {
                        eprintln!(
                            "clipboard_history: history file corrupt — quarantined to {quarantine:?}, starting fresh"
                        );
                        // Park a one-time notice so the in-app Clipboard page
                        // can tell the user their history was reset (and where
                        // the old bytes went) the next time it opens.
                        let name = quarantine
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("history.corrupt.json")
                            .to_string();
                        *recovery_notice()
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(name);
                    }
                    Err(error) => eprintln!(
                        "clipboard_history: history file corrupt and quarantine rename failed: {error}"
                    ),
                }
            }
        }
    }
    // Exclusions
    if let Ok(path) = exclusions_path(app) {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(payload) = serde_json::from_slice::<ExclusionsState>(&bytes) {
                let mut guard = locked_state();
                guard.exclusions = payload.apps.iter().map(|s| s.to_lowercase()).collect();
            }
        }
    }

    // Sweep orphan PNGs from the image cache. If the app crashed between
    // saving a PNG and persisting the entry that references it, the file
    // can sit on disk forever — never referenced by any entry, never
    // evicted by the count-cap or budget enforcer. Across many runs this
    // grows into a long tail of useless bytes. Running this once at
    // startup converges any inconsistency back to "every file has a
    // matching entry, every entry has a matching file".
    //
    // Also runs the disk-budget enforcer afterward: if the loaded entries
    // already exceed the budget (e.g. from an earlier version that didn't
    // enforce it), we trim down to budget here rather than waiting for
    // the next clipboard capture to trigger eviction.
    sweep_orphan_image_files(app);
    {
        let mut guard = locked_state();
        evict_to_image_budget(&mut guard);
    }
}

/// One-shot orphan cleanup for the clipboard image cache. Lists every
/// file in the image dir, builds a set of paths referenced by current
/// entries, and deletes anything not referenced. Errors are swallowed
/// (best-effort — the next sweep will catch what we missed). Bounded
/// in cost by the size of the directory; no recursion.
fn sweep_orphan_image_files(app: &AppHandle) {
    let Ok(images_dir) = app.path().app_data_dir().map(|d| d.join(IMAGE_STORAGE_SUBDIR)) else {
        return;
    };
    if !images_dir.exists() {
        return;
    }

    // Build the "files we're allowed to keep" set from current entries.
    // We canonicalize so case/separator differences don't cause false
    // positives. Skip entries with no path or invalid canonicalization.
    let referenced: std::collections::HashSet<PathBuf> = {
        let guard = locked_state();
        guard
            .entries
            .iter()
            .flat_map(|e| [e.image_path.as_ref(), e.thumbnail_path.as_ref()])
            .flatten()
            .filter_map(|p| p.canonicalize().ok())
            .collect()
    };

    let Ok(read_dir) = std::fs::read_dir(&images_dir) else {
        return;
    };

    let mut deleted = 0usize;
    for entry in read_dir.flatten() {
        let path = entry.path();
        // Only touch our own files — leave other artifacts alone.
        //
        // Must cover EVERY extension push_image_entry writes, not just png:
        // format-preserving capture stores gif/jpg/webp/bmp too, and while
        // this only matched "png" those four leaked forever — invisible,
        // because the disk-budget enforcer counts referenced entries and an
        // orphan is by definition unreferenced. Keep in sync with the `ext`
        // mapping in push_image_entry.
        let is_ours = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("png" | "jpg" | "gif" | "webp" | "bmp")
        );
        if !is_ours {
            continue;
        }
        let canonical = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => continue,
        };
        if !referenced.contains(&canonical) {
            if std::fs::remove_file(&canonical).is_ok() {
                deleted += 1;
            }
        }
    }

    if deleted > 0 {
        eprintln!("clipboard_history: swept {deleted} orphan image file(s) from cache");
    }
}

// ─── Entry management ────────────────────────────────────────────────────

/// Search: what's known about a copy the instant Windows announces it
/// (WM_CLIPBOARDUPDATE), before the settle delay. Who copied and whether it
/// asked not to be kept are decided from this, not from what's true 60 ms
/// later — a password manager that hides itself right after copying has
/// handed the foreground to someone else by then.
#[derive(Debug, Clone, Default, PartialEq)]
struct CopySnapshot {
    /// Our own write coming back (`arm_self_write_suppression`).
    ours: bool,
    source_app: Option<String>,
    source_app_path: Option<String>,
    /// `ExcludeClipboardContentFromMonitorProcessing` was on the clipboard.
    excluded: bool,
    /// `CanIncludeInClipboardHistory` was on the clipboard; its value is read
    /// with the data.
    history_marker: bool,
    /// GetClipboardSequenceNumber at the time: data read under a different
    /// number belongs to a later copy, which has its own snapshot coming.
    sequence: u32,
}

/// The latest copy's snapshot, waiting for the settle timer. A burst of
/// updates keeps only the last: that's the copy whose data will be read.
static PENDING_COPY: Mutex<Option<CopySnapshot>> = Mutex::new(None);

/// Whether a copy described by `snapshot` may be kept, given the app
/// exclusions and the value of `CanIncludeInClipboardHistory` read with the
/// data (`None`: absent or unreadable).
fn snapshot_allows_capture(snapshot: &CopySnapshot, exclusions: &[String], history_value: Option<&[u8]>) -> bool {
    if snapshot.ours || snapshot.excluded {
        return false;
    }
    if app_matches_exclusion(snapshot.source_app.as_deref(), exclusions) {
        return false;
    }
    !history_marker_forbids(snapshot.history_marker, history_value)
}

/// `CanIncludeInClipboardHistory` is a DWORD: 0 asks every history (Windows'
/// own, and ours) to leave the copy alone. A marker that was there but can't
/// be read is taken as that too.
fn history_marker_forbids(present: bool, value: Option<&[u8]>) -> bool {
    if !present {
        return false;
    }
    match value {
        Some(bytes) if bytes.len() >= 4 => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == 0,
        _ => true,
    }
}

/// Taken on WM_CLIPBOARDUPDATE itself: presence checks and the owner's
/// process only — nothing that opens the clipboard, which the copying app
/// may still need.
#[cfg(windows)]
fn snapshot_copy() -> CopySnapshot {
    use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
    let sequence = unsafe { GetClipboardSequenceNumber() };
    let ours = {
        let mut guard = locked_state();
        take_self_write(&mut guard.suppress_until_ms, now_ms())
    };
    if ours {
        return CopySnapshot { ours, sequence, ..Default::default() };
    }
    let (source_app, source_app_path) = clipboard_source_process();
    CopySnapshot {
        ours,
        source_app,
        source_app_path,
        excluded: clipboard_marked_no_capture(),
        history_marker: format_present(registered_format(&FORMAT_HISTORY_MARKER, "CanIncludeInClipboardHistory")),
        sequence,
    }
}

#[cfg(windows)]
fn format_present(format_id: u32) -> bool {
    use windows::Win32::System::DataExchange::IsClipboardFormatAvailable;
    format_id != 0 && unsafe { IsClipboardFormatAvailable(format_id) }.is_ok()
}

/// Whether the clipboard still holds the copy `snapshot` describes.
#[cfg(windows)]
fn same_copy(snapshot: &CopySnapshot) -> bool {
    use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
    unsafe { GetClipboardSequenceNumber() == snapshot.sequence }
}

/// The settle timer fired: read the copy the last update announced.
#[cfg(windows)]
fn on_settled() {
    let snapshot = PENDING_COPY.lock().unwrap_or_else(|p| p.into_inner()).take();
    if let Some(snapshot) = snapshot {
        on_clipboard_change(snapshot);
    }
}

/// Core "we saw a new clipboard event" path. Tries text first (most copies
/// are text); falls through to image capture if images are enabled and no
/// text was found. Writes the entry, schedules a save, emits the change
/// event. Called from the Win32 WindowProc once the copy has settled, with
/// what was known about it when it was announced.
#[cfg(windows)]
fn on_clipboard_change(snapshot: CopySnapshot) {
    // Our own write back to the clipboard is never a new entry.
    if snapshot.ours {
        return;
    }
    let (images_enabled, exclusions) = {
        let guard = locked_state();
        if guard.paused {
            return;
        }
        (guard.images_enabled, guard.exclusions.clone())
    };
    // A later copy has already replaced this one: its own update reads it.
    if !same_copy(&snapshot) {
        return;
    }
    // Honour the OS "exclude from clipboard monitors" marker (password fields
    // and secure inputs set it even when the source process itself is not on
    // the exclusion list), CanIncludeInClipboardHistory=0, and the list of
    // apps whose copies are never kept.
    let history_value = if snapshot.history_marker {
        read_clipboard_raw_bytes(registered_format(&FORMAT_HISTORY_MARKER, "CanIncludeInClipboardHistory"))
    } else {
        None
    };
    if !snapshot_allows_capture(&snapshot, &exclusions, history_value.as_deref()) {
        return;
    }
    let CopySnapshot { source_app, source_app_path, .. } = snapshot.clone();

    // Text path — most clipboard events. If text is present we take it and
    // don't also check for image data (even if the source app put both
    // formats on the clipboard, e.g. screenshots-with-alt-text).
    if let Some(mut text) = read_clipboard_text() {
        if !same_copy(&snapshot) {
            return;
        }
        if !text.trim().is_empty() {
            if text.len() > MAX_ENTRY_BYTES {
                text.truncate(floor_char_boundary(&text, MAX_ENTRY_BYTES));
            }
            let sensitive_kinds: Vec<String> = sensitive_scan::scan_text(&text)
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            let category = categorize(&text);
            push_text_entry(text, source_app, source_app_path, sensitive_kinds, category);
            schedule_save();
            emit_history_updated();
            return;
        }
    }

    // Image path — only when explicitly opted in. We never silently store
    // image data without consent.
    //
    // Priority order tries to preserve the source format losslessly:
    //   1. image/gif  → animation preserved, save bytes as .gif
    //   2. image/png  → lossless raw bytes, save as .png (avoids the
    //                   PNG-to-DIB-to-PNG round-trip that CF_DIB would force)
    //   3. image/jpeg → preserves JPEG quality bytes, save as .jpg
    //   4. image/webp → preserves WebP (incl. animated) bytes, save as .webp
    //   5. CF_DIB     → fallback for apps that only put the standard format.
    //                   We transcode to PNG for our own storage.
    //
    // Most modern apps (browsers, Discord, Snipping Tool, Photoshop) write
    // multiple formats; the registered MIME format ALWAYS wins so we pick
    // up the highest-fidelity representation.
    if images_enabled {
        let gif_id = registered_format(&FORMAT_GIF, "image/gif");
        let png_id = registered_format(&FORMAT_PNG, "image/png");
        let jpeg_id = registered_format(&FORMAT_JPEG, "image/jpeg");
        let webp_id = registered_format(&FORMAT_WEBP, "image/webp");

        let candidates: &[(u32, &str)] = &[
            (gif_id, "gif"),
            (png_id, "png"),
            (jpeg_id, "jpeg"),
            (webp_id, "webp"),
        ];

        for &(fmt_id, ext) in candidates {
            if let Some(bytes) = read_clipboard_raw_bytes(fmt_id) {
                if !same_copy(&snapshot) {
                    return;
                }
                let (bytes, ext) = fit_captured_image(bytes, ext);
                push_image_entry(bytes, source_app.clone(), source_app_path.clone(), &ext);
                schedule_save();
                emit_history_updated();
                return;
            }
        }

        // Next fallback: CF_HDROP — the user copied a file in File Explorer.
        // Workaround for Chromium browsers that strip GIF animation when
        // you "Copy image": save the .gif first, then copy the file from
        // Explorer; we read the bytes off disk preserving the format.
        let dropped_paths = read_clipboard_file_paths();
        for path in &dropped_paths {
            if let Some(format) = image_format_from_path(path) {
                match std::fs::read(path) {
                    Ok(bytes) => {
                        let (bytes, format) = fit_captured_image(bytes, format);
                        push_image_entry(bytes, source_app.clone(), source_app_path.clone(), &format);
                        schedule_save();
                        emit_history_updated();
                        return;
                    }
                    Err(error) => {
                        eprintln!(
                            "clipboard_history: failed to read dropped file {:?}: {error}",
                            path
                        );
                        // Try the next path (multi-file copy).
                    }
                }
            }
        }

        // Final fallback: CF_DIB (BMP-style bitmap → transcoded to PNG).
        // This is what a Chromium "Copy image on a GIF" lands on after the
        // earlier paths missed — Chrome rasterizes the current frame into
        // CF_DIB and into image/png, so we get a static snapshot. The
        // animated bytes were never on the clipboard to begin with.
        if let Some(png_bytes) = read_clipboard_image_as_png() {
            if !same_copy(&snapshot) {
                return;
            }
            let (png_bytes, format) = fit_captured_image(png_bytes, "png");
            push_image_entry(png_bytes, source_app, source_app_path, &format);
            schedule_save();
            emit_history_updated();
        }
    }
}

/// Map a file extension to one of our supported image format strings.
/// Returns None if the extension isn't a recognized image format — that's
/// what filters out copied PDFs / docs / random files from the CF_HDROP
/// pipeline.
fn image_format_from_path(path: &std::path::Path) -> Option<&'static str> {
    let ext = path.extension().and_then(|e| e.to_str())?.to_lowercase();
    match ext.as_str() {
        "gif" => Some("gif"),
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpeg"),
        "webp" => Some("webp"),
        "bmp" => Some("bmp"),
        _ => None,
    }
}

/// Pure exclusion check — `true` if `app_name` (matched case-insensitively)
/// is on the `exclusions` list, which is stored already-lowercased. Pure, so
/// the matching is unit-testable without global state.
fn app_matches_exclusion(app_name: Option<&str>, exclusions: &[String]) -> bool {
    let Some(name) = app_name else {
        return false;
    };
    let lower = name.to_lowercase();
    exclusions.iter().any(|e| e == &lower)
}

/// True when the app that owns the current clipboard contents asked monitors
/// not to store them. Password fields and secure inputs place the registered
/// `ExcludeClipboardContentFromMonitorProcessing` format on the clipboard;
/// honouring it keeps secrets out of the history even when the source app is
/// not on the exclusion list (e.g. a password field inside a browser).
#[cfg(windows)]
fn clipboard_marked_no_capture() -> bool {
    use windows::Win32::System::DataExchange::IsClipboardFormatAvailable;
    let format_id = registered_format(
        &FORMAT_EXCLUDE_MONITOR,
        "ExcludeClipboardContentFromMonitorProcessing",
    );
    if format_id == 0 {
        return false;
    }
    // IsClipboardFormatAvailable is a presence check — it needs no
    // OpenClipboard and races nothing the readers below do.
    unsafe { IsClipboardFormatAvailable(format_id) }.is_ok()
}

fn push_text_entry(
    text: String,
    source_app: Option<String>,
    source_app_path: Option<String>,
    sensitive_kinds: Vec<String>,
    category: Category,
) {
    let mut guard = locked_state();

    // Dedupe: if exactly this text is already anywhere in the ring, promote
    // it to the front and refresh metadata rather than create a duplicate.
    // Phase 1 only deduped against the immediate front; this catches the
    // common "I copied X, copied Y, copied X again" pattern too.
    if let Some(pos) = guard
        .entries
        .iter()
        .position(|e| matches!(e.kind, EntryKind::Text) && e.text == text)
    {
        // We just got `pos` from `position()` on this exact deque, so the
        // index is valid by construction. The defensive `if let Some(...)`
        // instead of `.expect()` means a future refactor that decouples
        // the position from the remove can never crash the daemon —
        // worst case, the dedup silently no-ops.
        if let Some(mut existing) = guard.entries.remove(pos) {
            existing.captured_at_ms = now_ms();
            existing.category = category;
            existing.sensitive_kinds = sensitive_kinds;
            if source_app.is_some() {
                existing.source_app = source_app;
            }
            if source_app_path.is_some() {
                existing.source_app_path = source_app_path;
            }
            guard.entries.push_front(existing);
            return;
        }
    }

    let id = guard.next_id;
    guard.next_id += 1;
    guard.entries.push_front(ClipboardEntry {
        id,
        captured_at_ms: now_ms(),
        kind: EntryKind::Text,
        text,
        source_app,
        source_app_path,
        sensitive_kinds,
        category,
        is_pinned: false,
        pin_label: None,
        image_path: None,
        thumbnail_path: None,
        image_width: None,
        image_height: None,
        image_size_bytes: None,
        image_format: None,
    });

    evict_to_cap(&mut guard);
}

/// Decode a static image, proportionally shrink it until its re-encoded PNG
/// fits under `MAX_IMAGE_BYTES`, and return the PNG bytes. Tries a ladder of
/// bounding boxes largest-first so the result keeps as much resolution as the
/// size cap allows. Returns `None` if the bytes can't be decoded at all.
fn downscale_static_image(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    for max_dim in [3840u32, 2560, 1920, 1280, 1024, 800] {
        let resized = img.resize(max_dim, max_dim, image::imageops::FilterType::Triangle);
        let mut encoded = Vec::new();
        let encoded_ok = resized
            .write_to(
                &mut std::io::Cursor::new(&mut encoded),
                image::ImageFormat::Png,
            )
            .is_ok();
        if encoded_ok && !encoded.is_empty() && encoded.len() <= MAX_IMAGE_BYTES {
            return Some(encoded);
        }
    }
    None
}

/// Bring a freshly-captured image under `MAX_IMAGE_BYTES` rather than dropping
/// the capture when it is too big. Static images (PNG / JPEG / BMP / the
/// CF_DIB transcode) are downscaled and re-encoded as PNG. Animated images
/// (GIF, animated WebP) are returned untouched — re-encoding them would
/// silently flatten the animation, so an oversized animation is kept as-is and
/// the cumulative disk-budget enforcer (`MAX_IMAGE_DISK_BYTES`) bounds the
/// cache instead. Returns the bytes to store plus the format string that
/// matches them — a downscaled static image becomes `"png"`.
fn fit_captured_image(bytes: Vec<u8>, format: &str) -> (Vec<u8>, String) {
    if bytes.len() <= MAX_IMAGE_BYTES {
        return (bytes, format.to_string());
    }
    // Animated formats: never re-encode — that would kill the animation.
    if matches!(format, "gif" | "webp") {
        return (bytes, format.to_string());
    }
    match downscale_static_image(&bytes) {
        Some(png) => (png, "png".to_string()),
        // Oversized but undecodable — keep the original rather than lose the
        // capture; the disk-budget enforcer still bounds total footprint.
        None => (bytes, format.to_string()),
    }
}

/// Generate a small PNG thumbnail of a captured image and write it next to
/// the full image as `<id>.thumb.png`. Returns the thumbnail path, or `None`
/// if the image can't be decoded or the write fails — the UI then falls back
/// to the full image. Animated GIF/WebP thumbnail their first frame, which is
/// exactly what a static list row wants; the full-size viewer still animates.
fn generate_thumbnail(full_bytes: &[u8], id: u64, dir: &std::path::Path) -> Option<PathBuf> {
    let img = image::load_from_memory(full_bytes).ok()?;
    let thumb = img.resize(
        THUMBNAIL_MAX_DIM,
        THUMBNAIL_MAX_DIM,
        image::imageops::FilterType::Triangle,
    );
    let mut encoded = Vec::new();
    thumb
        .write_to(
            &mut std::io::Cursor::new(&mut encoded),
            image::ImageFormat::Png,
        )
        .ok()?;
    let path = dir.join(format!("{id}.thumb.png"));
    std::fs::write(&path, &encoded).ok()?;
    Some(path)
}

/// Build an image entry. Writes the image bytes to disk under
/// `<app_data>/clipboard-images/<id>.<ext>`, records the metadata in the
/// ring, and never touches whatever was on the user's clipboard before.
///
/// `image_format` is one of `"gif"`, `"png"`, `"jpeg"`, `"webp"`, `"bmp"`.
/// The file extension matches so the asset protocol's MIME hint + browser
/// auto-detection both work correctly when the frontend renders the file
/// via `<img>` (animated GIFs animate, WebP renders, etc.). For paste-back
/// we re-write to the matching registered clipboard format so animation
/// survives the round trip.
///
/// No dedup across images — every image copy is its own entry. Computing
/// a perceptual hash to dedup *similar* screenshots isn't worth the
/// complexity for v1. Byte-identical repeats arriving in the same instant are
/// a different matter: those are one copy that Windows reported more than
/// once, and `is_duplicate_image_burst` drops them (see LAST_IMAGE).
fn push_image_entry(
    bytes: Vec<u8>,
    source_app: Option<String>,
    source_app_path: Option<String>,
    image_format: &str,
) {
    // Checked before the id allocation and the disk write below, so a repeat
    // event costs nothing but a hash — no orphaned file, no id burned.
    if is_duplicate_image_burst(&bytes) {
        return;
    }

    let app = match APP_HANDLE.get() {
        Some(a) => a.clone(),
        None => {
            eprintln!("clipboard_history: APP_HANDLE not set; skipping image capture");
            return;
        }
    };

    let id = {
        let mut guard = locked_state();
        let id = guard.next_id;
        guard.next_id += 1;
        id
    };

    let dir = match image_storage_dir(&app) {
        Ok(d) => d,
        Err(error) => {
            eprintln!("clipboard_history: image dir setup failed: {error}");
            return;
        }
    };
    // File extension matches the format so the asset protocol + browser
    // sniff the right MIME for inline rendering. JPEG conventionally uses
    // ".jpg" rather than ".jpeg" so we map that one.
    let ext = match image_format {
        "jpeg" => "jpg",
        other => other,
    };
    let path = dir.join(format!("{id}.{ext}"));
    if let Err(error) = std::fs::write(&path, &bytes) {
        eprintln!("clipboard_history: image write failed: {error}");
        return;
    }

    // image_dimensions sniffs format by content, not extension — works for
    // GIF / PNG / JPEG / WebP / BMP equally. Returns the first frame's
    // dimensions for animated formats, which is exactly what we want for UI.
    let (width, height) = image::image_dimensions(&path).unwrap_or((0, 0));

    // Generate the list-view thumbnail now, while we still hold the bytes in
    // memory. Failure is non-fatal — the entry is kept and the UI falls back
    // to the full image.
    let thumbnail_path = generate_thumbnail(&bytes, id, &dir);

    let mut guard = locked_state();
    guard.entries.push_front(ClipboardEntry {
        id,
        captured_at_ms: now_ms(),
        kind: EntryKind::Image,
        text: String::new(),
        source_app,
        source_app_path,
        sensitive_kinds: Vec::new(),
        category: Category::Image,
        is_pinned: false,
        pin_label: None,
        image_path: Some(path),
        thumbnail_path,
        image_width: Some(width),
        image_height: Some(height),
        image_size_bytes: Some(bytes.len() as u64),
        image_format: Some(image_format.to_string()),
    });

    evict_to_cap(&mut guard);
    // Run the disk-budget enforcer AFTER the count-based cap so the two
    // policies compose: count first culls oldest non-pinned regardless of
    // type, then the disk-budget enforcer further evicts oldest non-pinned
    // IMAGE entries if total bytes are still over the cap. Pinned items
    // are immune from both passes.
    evict_to_image_budget(&mut guard);
}

/// Trim the back of the ring to MAX_ENTRIES, skipping pinned items. Files
/// belonging to evicted image entries are unlinked from disk too — leaving
/// orphan PNGs would silently accumulate forever.
fn evict_to_cap(guard: &mut std::sync::MutexGuard<'_, State>) {
    while guard.entries.len() > MAX_ENTRIES {
        let pos = guard.entries.iter().rposition(|e| !e.is_pinned);
        match pos {
            Some(idx) => {
                if let Some(removed) = guard.entries.remove(idx) {
                    delete_image_file_for_entry(&removed);
                }
            }
            None => break,
        }
    }
}

/// Enforce the cumulative on-disk image budget. Sums `image_size_bytes`
/// across all image entries (pinned + non-pinned) and, if over budget,
/// evicts oldest non-pinned image entries until under the cap.
///
/// Why this matters: a heavy image-clipboard workflow (screenshots,
/// "copy image" from browsers) silently grows the cache. Without this
/// enforcement, the app data directory can balloon into multi-GB
/// territory over weeks, leaving users wondering why KeepItLocal is
/// eating their disk.
///
/// Pinned images count toward the total but are never evicted — if
/// the user's pinned set alone exceeds the budget, the function leaves
/// it untouched (we honor the explicit pin over the implicit budget).
fn evict_to_image_budget(guard: &mut std::sync::MutexGuard<'_, State>) {
    let total: u64 = guard
        .entries
        .iter()
        .filter_map(|e| e.image_size_bytes)
        .sum();
    if total <= MAX_IMAGE_DISK_BYTES {
        return;
    }

    let mut current = total;
    while current > MAX_IMAGE_DISK_BYTES {
        // Find the OLDEST non-pinned image entry (rposition because
        // entries are pushed to the front; back of the deque = oldest).
        let pos = guard.entries.iter().rposition(|e| {
            !e.is_pinned && matches!(e.kind, EntryKind::Image) && e.image_size_bytes.is_some()
        });
        let Some(idx) = pos else { break };
        let Some(removed) = guard.entries.remove(idx) else {
            break;
        };
        let freed = removed.image_size_bytes.unwrap_or(0);
        delete_image_file_for_entry(&removed);
        // Saturating subtract because the recorded size could be stale
        // (e.g. file was already manually deleted before this run).
        current = current.saturating_sub(freed);
    }
}

// ─── Tauri commands ──────────────────────────────────────────────────────

#[tauri::command]
pub fn start_clipboard_listener(app: AppHandle) -> Result<(), String> {
    if LISTENER_STARTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let _ = APP_HANDLE.set(app.clone());
    load_from_disk(&app);

    // Spawn the Win32 message-pump thread (event-driven, zero idle CPU).
    let app_for_listener = app.clone();
    std::thread::Builder::new()
        .name("keepitlocal-clipboard-listener-supervisor".to_string())
        .spawn(move || run_listener_supervisor(app_for_listener))
        .map_err(|e| format!("spawn clipboard listener: {e}"))?;

    // Spawn the debounced save thread.
    let app_for_saver = app;
    std::thread::Builder::new()
        .name("keepitlocal-clipboard-saver".to_string())
        .spawn(move || run_save_thread(app_for_saver))
        .map_err(|e| format!("spawn clipboard saver: {e}"))?;

    Ok(())
}

#[tauri::command]
pub fn get_clipboard_history() -> Vec<ClipboardEntry> {
    locked_state().entries.iter().cloned().collect()
}

/// Pull — and clear — any pending corrupt-history recovery notice. Returns
/// the quarantine filename exactly once after a recovery (then `None`), so
/// the in-app Clipboard page surfaces the "history was reset" banner a single
/// time rather than on every mount. Called on page mount.
#[tauri::command]
pub fn take_clipboard_recovery_notice() -> Option<String> {
    recovery_notice()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
}

#[tauri::command]
pub fn clear_clipboard_history() -> Result<(), String> {
    let removed: Vec<ClipboardEntry> = {
        let mut guard = locked_state();
        let mut removed = Vec::new();
        guard.entries.retain(|e| {
            if e.is_pinned {
                true
            } else {
                removed.push(e.clone());
                false
            }
        });
        removed
    };
    // Remove on-disk images that belonged to non-pinned entries. The lock is
    // released before this; we don't want disk I/O blocking the state mutex.
    for entry in &removed {
        delete_image_file_for_entry(entry);
    }
    schedule_save();
    emit_history_updated();
    Ok(())
}

#[tauri::command]
pub fn pin_clipboard_entry(id: u64, pinned: bool) -> Result<(), String> {
    {
        let mut guard = locked_state();
        if let Some(entry) = guard.entries.iter_mut().find(|e| e.id == id) {
            entry.is_pinned = pinned;
            if !pinned {
                // Unpinning drops the label too; pin_label has no meaning
                // for non-pinned entries.
                entry.pin_label = None;
            }
        }
    }
    schedule_save();
    emit_history_updated();
    Ok(())
}

/// Set or clear the optional label on a pinned entry. Label is shown in the
/// UI in place of the content preview so users can recognize "my email
/// signature" without scanning the actual bytes. Only meaningful for pinned
/// entries — silently ignored for transient ones.
#[tauri::command]
pub fn label_clipboard_entry(id: u64, label: Option<String>) -> Result<(), String> {
    {
        let mut guard = locked_state();
        if let Some(entry) = guard.entries.iter_mut().find(|e| e.id == id) {
            if entry.is_pinned {
                entry.pin_label = label.filter(|s| !s.trim().is_empty());
            }
        }
    }
    schedule_save();
    emit_history_updated();
    Ok(())
}

#[tauri::command]
pub fn delete_clipboard_entry(id: u64) -> Result<(), String> {
    // Capture the entry (and its image path if any) before removing it from
    // the ring, so we can delete the on-disk file after releasing the lock.
    let removed = {
        let mut guard = locked_state();
        let position = guard.entries.iter().position(|e| e.id == id);
        position.and_then(|idx| guard.entries.remove(idx))
    };
    if let Some(entry) = removed.as_ref() {
        delete_image_file_for_entry(entry);
    }
    schedule_save();
    emit_history_updated();
    Ok(())
}

/// Bulk-delete entries by id — one lock acquisition and one change event for
/// the whole batch, so a 50-item bulk delete does not trigger 50 separate
/// history refreshes in the UI. Image files for removed image entries are
/// unlinked, same as the single-id path. Unknown ids are silently ignored.
#[tauri::command]
pub fn delete_clipboard_entries(ids: Vec<u64>) -> Result<(), String> {
    let id_set: std::collections::HashSet<u64> = ids.into_iter().collect();
    if id_set.is_empty() {
        return Ok(());
    }
    let removed: Vec<ClipboardEntry> = {
        let mut guard = locked_state();
        let mut removed = Vec::new();
        guard.entries.retain(|e| {
            if id_set.contains(&e.id) {
                removed.push(e.clone());
                false
            } else {
                true
            }
        });
        removed
    };
    // File cleanup outside the lock — disk I/O should not block the listener.
    for entry in &removed {
        delete_image_file_for_entry(entry);
    }
    if !removed.is_empty() {
        schedule_save();
        emit_history_updated();
    }
    Ok(())
}

/// Bulk pin/unpin entries by id — one lock and one change event for the
/// batch. Unpinning clears each entry's label (consistent with the single-id
/// `pin_clipboard_entry`, since `pin_label` has no meaning for non-pinned
/// entries). Unknown ids are silently ignored.
#[tauri::command]
pub fn pin_clipboard_entries(ids: Vec<u64>, pinned: bool) -> Result<(), String> {
    let id_set: std::collections::HashSet<u64> = ids.into_iter().collect();
    if id_set.is_empty() {
        return Ok(());
    }
    {
        let mut guard = locked_state();
        for entry in guard.entries.iter_mut() {
            if id_set.contains(&entry.id) {
                entry.is_pinned = pinned;
                if !pinned {
                    entry.pin_label = None;
                }
            }
        }
    }
    schedule_save();
    emit_history_updated();
    Ok(())
}

/// Promote a past entry back to the OS clipboard. The user then pastes
/// manually with Ctrl+V. Kept around as a no-side-effects path — most
/// callers should use `paste_clipboard_entry` (with auto_paste=true) for
/// the full "magic" experience where the keystroke is also injected.
///
/// Dispatches on entry kind: text entries write CF_UNICODETEXT, image
/// entries decode the on-disk PNG and write CF_DIB so the target app sees
/// it as a regular image paste.
///
/// Search: `quiet` marks a text copy the way password managers mark theirs
/// (`ExcludeClipboardContentFromMonitorProcessing`,
/// `CanIncludeInClipboardHistory=0`, `CanUploadToCloudClipboard=0`), so a
/// secret put back isn't kept by Windows' history, its cloud clipboard or
/// any other clipboard manager.
#[tauri::command]
pub fn copy_clipboard_entry_to_clipboard(id: u64, quiet: Option<bool>) -> Result<(), String> {
    let snapshot = entry_snapshot(id)?;
    arm_self_write_suppression();
    let result = match snapshot.kind {
        EntryKind::Text => write_text_to_clipboard_marked(&snapshot.text, quiet.unwrap_or(false)),
        EntryKind::Image => match snapshot.image_path.as_ref() {
            Some(path) => write_image_to_clipboard(path, snapshot.image_format.as_deref()),
            None => Err("Image entry has no on-disk file".to_string()),
        },
    };
    result.map_err(|error| {
        clear_self_write_suppression();
        error
    })
}

/// The "Enter" behavior — pulls together the full paste pipeline:
///   1. Write the selected entry's text back to the OS clipboard.
///   2. Refocus the window that was active just before the overlay opened
///      (captured by `remember_foreground_window` at show time).
///   3. Inject a synthetic Ctrl+V keystroke into that window.
///
/// `auto_paste = Some(false)` falls back to copy-only — useful when the
/// user has disabled the feature, or when we couldn't capture a target
/// window (e.g. overlay was opened via tray click, not the global hotkey).
///
/// Failure modes are intentionally tolerant: if SetForegroundWindow fails
/// (Win32 has restrictions on which processes can steal focus), the text
/// still lives on the clipboard so the user can Ctrl+V manually.
#[tauri::command]
pub fn paste_clipboard_entry(id: u64, auto_paste: Option<bool>) -> Result<(), String> {
    let snapshot = entry_snapshot(id)?;
    arm_self_write_suppression();
    let write_result = match snapshot.kind {
        EntryKind::Text => write_text_to_clipboard(&snapshot.text),
        EntryKind::Image => match snapshot.image_path.as_ref() {
            Some(path) => write_image_to_clipboard(path, snapshot.image_format.as_deref()),
            None => Err("Image entry has no on-disk file".to_string()),
        },
    };
    if let Err(error) = write_result {
        clear_self_write_suppression();
        return Err(error);
    }

    let should_auto_paste = auto_paste.unwrap_or(true);
    if !should_auto_paste {
        return Ok(());
    }

    #[cfg(windows)]
    {
        let target = PREVIOUS_FOREGROUND_HWND.load(Ordering::SeqCst);
        if target == 0 {
            // Overlay was opened from somewhere we didn't capture a target —
            // typically tray-click or a programmatic show. Auto-paste isn't
            // possible; tell the user the text is at least on the clipboard.
            emit_paste_fallback(
                "Text is on your clipboard — switch to your target app and press Ctrl+V.",
            );
            return Ok(());
        }
        // SetForegroundWindow + SendInput need the overlay to first
        // release focus. We spawn a tiny detached thread that sleeps
        // ~30ms before firing — long enough that the OS has actually
        // moved focus away from the overlay (it was hidden by the
        // frontend just before calling this command). Without this,
        // Ctrl+V occasionally hits the overlay window in its brief
        // dying-focus state.
        std::thread::Builder::new()
            .name("keepitlocal-paste-injector".to_string())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(30));
                focus_and_paste(target);
            })
            .map_err(|e| format!("spawn paste injector: {e}"))?;
    }
    Ok(())
}

/// Paste a snippet expansion's body using the same clipboard + SendInput
/// pipeline as `paste_clipboard_entry`. Caller provides the already-
/// expanded text (variables resolved by the snippets module). Mirrors
/// `paste_clipboard_entry` but with raw text in lieu of a history-entry
/// lookup — keeps snippet expansions and history pastes on identical
/// focus-restore + injection paths so they feel identical to the user.
#[tauri::command]
pub fn paste_snippet_text(
    text: String,
    auto_paste: Option<bool>,
    restore_clipboard: Option<bool>,
) -> Result<(), String> {
    // #22 — voice dictation can ask for the user's prior clipboard text
    // to be put back after the paste so dictating doesn't clobber it.
    // Snapshot it BEFORE we overwrite; only text content is restorable
    // (an image / files on the clipboard yield None — the dictated text
    // then stays, the pre-#22 behaviour).
    #[cfg(windows)]
    let prior_clipboard: Option<String> = if restore_clipboard.unwrap_or(false) {
        read_clipboard_text().filter(|t| !t.is_empty())
    } else {
        None
    };
    #[cfg(not(windows))]
    let _ = restore_clipboard;

    arm_self_write_suppression();
    if let Err(error) = write_text_to_clipboard(&text) {
        clear_self_write_suppression();
        return Err(error);
    }

    let should_auto_paste = auto_paste.unwrap_or(true);
    if !should_auto_paste {
        return Ok(());
    }

    #[cfg(windows)]
    {
        let target = PREVIOUS_FOREGROUND_HWND.load(Ordering::SeqCst);
        if target == 0 {
            emit_paste_fallback(
                "Snippet is on your clipboard — switch to your target app and press Ctrl+V.",
            );
            return Ok(());
        }
        std::thread::Builder::new()
            .name("keepitlocal-snippet-paste-injector".to_string())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(30));
                focus_and_paste(target);
                // #22 — once the target app has had time to consume the
                // Ctrl+V, put the user's prior clipboard text back.
                if let Some(prior) = prior_clipboard {
                    std::thread::sleep(Duration::from_millis(400));
                    arm_self_write_suppression();
                    if write_text_to_clipboard(&prior).is_err() {
                        clear_self_write_suppression();
                    }
                }
            })
            .map_err(|e| format!("spawn snippet paste injector: {e}"))?;
    }
    Ok(())
}

/// Drop `text` onto the clipboard as a last-resort rescue when the type-out
/// path could not deliver it to the target window. History capture is
/// suppressed — the user picked type-out specifically to keep dictation OUT
/// of clipboard history, so the rescue copy must not be recorded there
/// either. The point is only that the text is never lost: worst case is a
/// manual Ctrl+V.
#[cfg(windows)]
fn rescue_text_to_clipboard(text: &str) {
    arm_self_write_suppression();
    if write_text_to_clipboard(text).is_err() {
        // The write failed, so the clipboard listener will never observe it
        // and consume the suppression flag — clear it ourselves so the next
        // genuine copy isn't silently dropped from history.
        clear_self_write_suppression();
    }
}

/// Insert `text` into the previously-focused window as synthetic keystrokes —
/// the "type-out" dictation output mode. Unlike `paste_snippet_text`, the
/// success path never touches the clipboard, so dictating does not overwrite
/// whatever the user had copied (and dictation never lands in clipboard
/// history). Used by the voice overlay and push-to-talk when the voice
/// output mode is set to "type". On any failure the text is rescued onto the
/// clipboard so it is never lost.
#[tauri::command]
pub fn type_out_text(text: String) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    #[cfg(windows)]
    {
        let target = PREVIOUS_FOREGROUND_HWND.load(Ordering::SeqCst);
        if target == 0 {
            // No remembered window (lock screen, a UAC prompt was up, etc.)
            // — there is nothing to type into. Rescue to the clipboard.
            rescue_text_to_clipboard(&text);
            emit_paste_fallback(
                "Couldn't find the target window — your dictation is on the clipboard, press Ctrl+V to paste.",
            );
            return Ok(());
        }
        std::thread::Builder::new()
            .name("keepitlocal-voice-typeout-injector".to_string())
            .spawn(move || {
                // Same 30 ms beat as the paste injector: gives the caller's
                // window time to hide so focus reverts to the user's app
                // before the keystrokes are dispatched.
                std::thread::sleep(Duration::from_millis(30));
                focus_and_type(target, text);
            })
            .map_err(|e| format!("spawn voice typeout injector: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        // Keystroke synthesis is Windows-only for now — mirrors the
        // `paste_snippet_text` paste path being a `#[cfg(windows)]` block.
        let _ = text;
    }
    Ok(())
}

/// Capture the foreground window just before the overlay takes focus, so
/// `paste_clipboard_entry` can refocus that window and inject Ctrl+V into
/// it. Called from `show_clipboard_overlay_window` in lib.rs. Idempotent —
/// repeated calls just overwrite the saved HWND.
#[cfg(windows)]
pub fn remember_foreground_window() {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe {
        let hwnd = GetForegroundWindow();
        // 0 means "no foreground" (lock screen, UAC, etc.) — stored as-is
        // so the paste path knows to skip the refocus step.
        PREVIOUS_FOREGROUND_HWND.store(hwnd.0, Ordering::SeqCst);
    }
}

#[cfg(not(windows))]
pub fn remember_foreground_window() {
    // No-op on non-Windows — paste simulation is Windows-only for now.
}

/// Pull a small snapshot of an entry's payload-relevant fields. Cloning the
/// whole ClipboardEntry would be wasteful for image entries (text/sensitive
/// vecs are empty there anyway), so we lift just what we need. Returns a
/// helper struct rather than the full entry to make callsites read clearly.
struct EntrySnapshot {
    kind: EntryKind,
    text: String,
    image_path: Option<PathBuf>,
    image_format: Option<String>,
}

fn entry_snapshot(id: u64) -> Result<EntrySnapshot, String> {
    let guard = locked_state();
    guard
        .entries
        .iter()
        .find(|e| e.id == id)
        .map(|e| EntrySnapshot {
            kind: e.kind,
            text: e.text.clone(),
            image_path: e.image_path.clone(),
            image_format: e.image_format.clone(),
        })
        .ok_or_else(|| format!("clipboard entry {id} not found"))
}

/// How long our own write may take to come back as a WM_CLIPBOARDUPDATE.
const SELF_WRITE_WINDOW_MS: i64 = 500;

fn arm_self_write_suppression() {
    locked_state().suppress_until_ms = now_ms() + SELF_WRITE_WINDOW_MS;
}

fn clear_self_write_suppression() {
    locked_state().suppress_until_ms = 0;
}

/// Whether the update seen at `now` is our own write: true once, while the
/// deadline hasn't passed; a passed deadline is cleared either way.
fn take_self_write(until_ms: &mut i64, now: i64) -> bool {
    let ours = *until_ms != 0 && now <= *until_ms;
    *until_ms = 0;
    ours
}

#[tauri::command]
pub fn get_clipboard_exclusions() -> Vec<String> {
    locked_state().exclusions.clone()
}

#[tauri::command]
pub fn set_clipboard_exclusions(apps: Vec<String>) -> Result<(), String> {
    let normalized: Vec<String> = apps
        .into_iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    {
        let mut guard = locked_state();
        guard.exclusions = normalized;
    }
    if let Some(app) = APP_HANDLE.get() {
        save_exclusions(app);
    }
    emit_history_updated();
    Ok(())
}

/// Reset the exclusion list to the built-in defaults. Useful escape hatch if
/// the user has mangled their list and wants the sensible password-manager
/// presets back without typing them in.
#[tauri::command]
pub fn reset_clipboard_exclusions_to_defaults() -> Result<(), String> {
    {
        let mut guard = locked_state();
        guard.exclusions = DEFAULT_EXCLUSIONS
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
    }
    if let Some(app) = APP_HANDLE.get() {
        save_exclusions(app);
    }
    emit_history_updated();
    Ok(())
}

/// The text currently on the clipboard, for resolving `{{clipboard}}` at
/// snippet-expansion time.
///
/// Deliberately NOT read from the history ring: `{{clipboard}}` means "what is
/// on the clipboard right now". History can be paused, the source app can be on
/// the exclusion list, or the copy can carry the OS "exclude from monitors"
/// marker — in all of those the ring is empty or stale while the clipboard is
/// not. Reading the live clipboard is the only answer that's always right.
///
/// Returns None when the clipboard holds no text (an image, or nothing).
#[tauri::command(async)]
pub fn get_clipboard_text() -> Option<String> {
    read_clipboard_text()
}

#[tauri::command]
pub fn get_clipboard_paused() -> bool {
    locked_state().paused
}

#[tauri::command]
pub fn set_clipboard_paused(paused: bool) -> Result<(), String> {
    locked_state().paused = paused;
    emit_history_updated();
    Ok(())
}

#[tauri::command]
pub fn get_clipboard_images_enabled() -> bool {
    locked_state().images_enabled
}

#[tauri::command]
pub fn set_clipboard_images_enabled(enabled: bool) -> Result<(), String> {
    locked_state().images_enabled = enabled;
    schedule_save();
    emit_history_updated();
    Ok(())
}

#[tauri::command]
pub fn get_clipboard_image_retention_days() -> u32 {
    locked_state().image_retention_days
}

/// Update the time-based retention threshold for image entries. Clamped to
/// 0..=90 because images are large and very long retention windows can
/// accumulate gigabytes on disk. `0` disables time-expiry for images.
#[tauri::command]
pub fn set_clipboard_image_retention_days(days: u32) -> Result<(), String> {
    let clamped = days.min(90);
    locked_state().image_retention_days = clamped;
    schedule_save();
    emit_history_updated();
    Ok(())
}

#[tauri::command]
pub fn get_clipboard_retention_days() -> u32 {
    locked_state().retention_days
}

/// Update the time-based retention threshold. `0` disables time-expiry (only
/// the 200-entry cap will apply). Clamped to 0..=365. Persisted so it
/// survives restarts.
#[tauri::command]
pub fn set_clipboard_retention_days(days: u32) -> Result<(), String> {
    let clamped = days.min(365);
    {
        let mut guard = locked_state();
        guard.retention_days = clamped;
    }
    schedule_save();
    emit_history_updated();
    Ok(())
}

// ─── Win32 listener (event-driven) ──────────────────────────────────────

/// Which listener window has a settle timer armed (0: none). Keyed by the
/// window rather than a plain flag, so a listener rebuilt after its thread
/// died mid-timer isn't left waiting forever for a timer that went with the
/// old window.
struct SettleGate(AtomicIsize);

impl SettleGate {
    const fn new() -> Self {
        Self(AtomicIsize::new(0))
    }

    /// A new listener window: nothing is armed any more.
    fn reset(&self) {
        self.0.store(0, Ordering::SeqCst);
    }

    /// An update arrived on `hwnd`: true when a timer must be armed for it,
    /// false while one already is.
    fn arm(&self, hwnd: isize) -> bool {
        self.0.swap(hwnd, Ordering::SeqCst) != hwnd
    }

    /// `hwnd`'s timer fired, or couldn't be set.
    fn done(&self, hwnd: isize) {
        let _ = self.0.compare_exchange(hwnd, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
}

#[cfg(windows)]
static SETTLE: SettleGate = SettleGate::new();

/// Module-level identifier for the message-only window class. Must be unique
/// per process and stable across calls. The "v1" suffix lets us version the
/// listener if its semantics ever change.
#[cfg(windows)]
const WINDOW_CLASS_NAME: &str = "KeepItLocal_ClipboardListener_v1";

#[cfg(windows)]
fn run_listener_thread(_app: AppHandle) {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::DataExchange::AddClipboardFormatListener;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer,
        RegisterClassExW, SetTimer, TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE,
        WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_TIMER, WNDCLASSEXW,
    };

    // Search: the copy is read a moment after Windows says it happened, not
    // at once. An app that puts delay-rendered data on the clipboard (OLE's
    // OleSetClipboard, then OleFlushClipboard) needs the clipboard again right
    // after announcing it; opening it in that instant made about one copy in
    // fifty fail in the copying app while history was on. One timer, armed by
    // the first update and not re-armed by the ones behind it, so a burst
    // still settles after SETTLE_MS.
    // Who copied, and whether it asked not to be kept, are taken at once
    // (snapshot_copy); only the reading of the data waits.
    const SETTLE_TIMER: usize = 1;
    const SETTLE_MS: u32 = 60;

    // Window procedure — receives messages from Windows. C calling convention,
    // no closures with captures. WindowProc reaches our state via the STATE
    // and APP_HANDLE statics.
    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_CLIPBOARDUPDATE {
            let snapshot = snapshot_copy();
            *PENDING_COPY.lock().unwrap_or_else(|p| p.into_inner()) = Some(snapshot);
            if SETTLE.arm(hwnd.0) && SetTimer(hwnd, SETTLE_TIMER, SETTLE_MS, None) == 0 {
                // No timer to be had: read it now, as before.
                SETTLE.done(hwnd.0);
                on_settled();
            }
            return LRESULT(0);
        }
        if msg == WM_TIMER && wparam.0 == SETTLE_TIMER {
            let _ = KillTimer(hwnd, SETTLE_TIMER);
            SETTLE.done(hwnd.0);
            on_settled();
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    // UTF-16 class name, null-terminated.
    let class_name_wide: Vec<u16> =
        WINDOW_CLASS_NAME.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(PCWSTR::null())
            .unwrap_or_default()
            .into();
        wc.lpszClassName = PCWSTR(class_name_wide.as_ptr());

        // Registration can legitimately fail if we're re-entering after the
        // class was already registered for this process. ATOM = 0 → already
        // registered; not fatal.
        let _ = RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name_wide.as_ptr()),
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            wc.hInstance,
            None,
        );
        if hwnd.0 == 0 {
            eprintln!("clipboard_history: CreateWindowExW failed");
            return;
        }
        // A window of our own: a timer the last one had armed died with it,
        // and so did the copy it was waiting on.
        SETTLE.reset();
        *PENDING_COPY.lock().unwrap_or_else(|p| p.into_inner()) = None;

        if AddClipboardFormatListener(hwnd).is_err() {
            eprintln!("clipboard_history: AddClipboardFormatListener failed");
            return;
        }

        // Message pump — GetMessageW blocks until a message arrives. Idle
        // CPU is literally 0% (the thread is parked by the OS).
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, HWND(0), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(windows))]
fn run_listener_thread(_app: AppHandle) {
    // Non-Windows clipboard listening is a separate platform integration
    // (Cocoa NSPasteboard polling on macOS, X11/Wayland on Linux). Phase 1
    // is Windows-first; macOS/Linux ports come later.
}

/// Supervises the Win32 clipboard-listener thread. `run_listener_thread`'s
/// message pump is meant to block forever; if it ever returns — the
/// message-only window died, `CreateWindowExW` / `AddClipboardFormatListener`
/// failed at startup, or the thread panicked — clipboard capture has silently
/// stopped. The supervisor runs each attempt on its own thread (so a panic is
/// caught via the join result rather than tearing down the supervisor) and
/// rebuilds the listener — fresh window + fresh `AddClipboardFormatListener` —
/// after an exponential backoff. A listener that ran healthily for a while
/// resets the backoff, so a rare one-off failure recovers fast while a hard,
/// immediate failure does not spin.
#[cfg(windows)]
fn run_listener_supervisor(app: AppHandle) {
    const MIN_BACKOFF_MS: u64 = 1_000;
    const MAX_BACKOFF_MS: u64 = 60_000;
    const HEALTHY_RUN_SECS: u64 = 30;

    let mut backoff_ms = MIN_BACKOFF_MS;
    loop {
        let app_for_attempt = app.clone();
        let started = std::time::Instant::now();
        let attempt = std::thread::Builder::new()
            .name("keepitlocal-clipboard-listener".to_string())
            .spawn(move || run_listener_thread(app_for_attempt));

        match attempt {
            Ok(handle) => match handle.join() {
                Ok(()) => {
                    eprintln!("clipboard_history: listener thread exited — rebuilding");
                }
                Err(_) => {
                    eprintln!("clipboard_history: listener thread panicked — rebuilding");
                }
            },
            Err(error) => {
                eprintln!("clipboard_history: cannot spawn listener thread: {error}");
            }
        }

        // A listener that survived a healthy stretch before dying is treated
        // as a one-off — reset the backoff so recovery is immediate next time.
        if started.elapsed() >= Duration::from_secs(HEALTHY_RUN_SECS) {
            backoff_ms = MIN_BACKOFF_MS;
        }
        std::thread::sleep(Duration::from_millis(backoff_ms));
        backoff_ms = backoff_ms.saturating_mul(2).min(MAX_BACKOFF_MS);
    }
}

#[cfg(not(windows))]
fn run_listener_supervisor(app: AppHandle) {
    // The non-Windows listener is an intentional no-op stub — nothing to
    // supervise until the macOS/Linux ports land.
    run_listener_thread(app);
}

// ─── Win32 helpers (clipboard read/write, foreground process) ────────────

/// Open the OS clipboard, retrying under contention. Another process can hold
/// the clipboard briefly while it finishes its own copy; rather than dropping
/// the capture on the first failure we retry with exponential backoff up to a
/// bounded ceiling (6 attempts, ~0.5 s total wait). Returns `true` once the
/// clipboard is open and owned by this thread — a `true` return MUST be paired
/// with a later `CloseClipboard`. Shared by every clipboard reader/writer so
/// the contention policy lives in exactly one place.
#[cfg(windows)]
fn open_clipboard_with_retry() -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::DataExchange::OpenClipboard;

    let mut delay_ms = 15u64;
    for attempt in 0..6 {
        if unsafe { OpenClipboard(HWND(0)) }.is_ok() {
            return true;
        }
        if attempt < 5 {
            std::thread::sleep(Duration::from_millis(delay_ms));
            delay_ms = (delay_ms * 2).min(240);
        }
    }
    false
}

#[cfg(windows)]
fn read_clipboard_text() -> Option<String> {
    use core::ffi::c_void;
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData};
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    if !open_clipboard_with_retry() {
        return None;
    }

    let result = unsafe {
        let handle: HANDLE = match GetClipboardData(CF_UNICODETEXT) {
            Ok(h) => h,
            Err(_) => {
                let _ = CloseClipboard();
                return None;
            }
        };
        if handle.0 == 0 {
            let _ = CloseClipboard();
            return None;
        }

        let hglobal = HGLOBAL(handle.0 as *mut c_void);
        let ptr = GlobalLock(hglobal) as *const u16;
        if ptr.is_null() {
            let _ = CloseClipboard();
            return None;
        }
        let byte_len = GlobalSize(hglobal);
        let wide_len = byte_len / 2;
        let slice = std::slice::from_raw_parts(ptr, wide_len);
        let trimmed: &[u16] = match slice.split(|&c| c == 0).next() {
            Some(s) => s,
            None => slice,
        };
        let text = String::from_utf16_lossy(trimmed);
        let _ = GlobalUnlock(hglobal);
        Some(text)
    };

    unsafe {
        let _ = CloseClipboard();
    }
    result
}

#[cfg(windows)]
fn write_text_to_clipboard(text: &str) -> Result<(), String> {
    write_text_to_clipboard_marked(text, false)
}

/// The formats a quiet copy carries besides its text, each a DWORD 0 (the
/// first is read by presence alone).
#[cfg(windows)]
const QUIET_MARKERS: [&str; 3] = [
    "ExcludeClipboardContentFromMonitorProcessing",
    "CanIncludeInClipboardHistory",
    "CanUploadToCloudClipboard",
];

/// Set one marker format on the open clipboard. Best effort: the text is
/// already there, and a monitor that sees one marker skips the copy.
#[cfg(windows)]
unsafe fn put_quiet_marker(name: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GlobalFree, HANDLE};
    use windows::Win32::System::DataExchange::{RegisterClipboardFormatW, SetClipboardData};
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let format = RegisterClipboardFormatW(PCWSTR(wide.as_ptr()));
    if format == 0 {
        return;
    }
    let Ok(hglobal) = GlobalAlloc(GMEM_MOVEABLE, 4) else { return };
    if hglobal.0.is_null() {
        return;
    }
    let ptr = GlobalLock(hglobal) as *mut u8;
    if ptr.is_null() {
        let _ = GlobalFree(hglobal);
        return;
    }
    std::ptr::write_bytes(ptr, 0, 4);
    let _ = GlobalUnlock(hglobal);
    if SetClipboardData(format, HANDLE(hglobal.0 as isize)).is_err() {
        let _ = GlobalFree(hglobal);
    }
}

#[cfg(windows)]
fn write_text_to_clipboard_marked(text: &str, quiet: bool) -> Result<(), String> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, SetClipboardData};
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    let mut wide: Vec<u16> = text.encode_utf16().collect();
    wide.push(0);
    let byte_len = wide.len() * std::mem::size_of::<u16>();

    if !open_clipboard_with_retry() {
        return Err("could not open clipboard (held by another app)".to_string());
    }
    unsafe {
        if EmptyClipboard().is_err() {
            let _ = CloseClipboard();
            return Err("EmptyClipboard failed".to_string());
        }

        let hglobal = GlobalAlloc(GMEM_MOVEABLE, byte_len)
            .map_err(|e| format!("GlobalAlloc failed: {e}"))?;
        if hglobal.0.is_null() {
            let _ = CloseClipboard();
            return Err("GlobalAlloc returned null".to_string());
        }

        let ptr = GlobalLock(hglobal) as *mut u16;
        if ptr.is_null() {
            let _ = CloseClipboard();
            return Err("GlobalLock failed".to_string());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
        let _ = GlobalUnlock(hglobal);

        let handle = HANDLE(hglobal.0 as isize);
        if SetClipboardData(CF_UNICODETEXT, handle).is_err() {
            let _ = CloseClipboard();
            return Err("SetClipboardData failed".to_string());
        }
        if quiet {
            for name in QUIET_MARKERS {
                put_quiet_marker(name);
            }
        }
        let _ = CloseClipboard();
    }
    Ok(())
}

/// Who put the copy on the clipboard: the process owning the clipboard
/// (GetClipboardOwner), or — when the copier opened it without a window —
/// whoever has the foreground.
#[cfg(windows)]
fn clipboard_source_process() -> (Option<String>, Option<String>) {
    use windows::Win32::System::DataExchange::GetClipboardOwner;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let owner = unsafe { GetClipboardOwner() };
    let hwnd = if owner.0 != 0 { owner } else { unsafe { GetForegroundWindow() } };
    (process_name_of(hwnd), process_path_of(hwnd))
}

/// The process behind a window, opened for querying; None for no window.
#[cfg(windows)]
fn process_of(hwnd: windows::Win32::Foundation::HWND) -> Option<windows::Win32::Foundation::HANDLE> {
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    unsafe {
        if hwnd.0 == 0 {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid).ok()
    }
}

#[cfg(windows)]
fn process_name_of(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;

    unsafe {
        let handle = process_of(hwnd)?;
        let mut buf = [0u16; 260];
        let len = GetModuleBaseNameW(handle, None, &mut buf);
        let _ = CloseHandle(handle);
        if len == 0 {
            return None;
        }
        let name = String::from_utf16_lossy(&buf[..len as usize]);
        // Strip the `.exe` suffix case-insensitively. Windows returns process
        // names in various cases — `WINWORD.EXE`, `chrome.exe`, `Code.exe` —
        // and a literal `.strip_suffix(".exe")` would miss the uppercase
        // variants, leaving raw `WINWORD.EXE` in the history for the frontend
        // to deal with. Doing it here means the rest of the pipeline (mapping
        // to "Microsoft Word", source-color hashing, etc.) sees a clean name.
        let cleaned = if name.len() >= 4
            && name.as_bytes()[name.len() - 4..].eq_ignore_ascii_case(b".exe")
        {
            name[..name.len() - 4].to_string()
        } else {
            name
        };
        Some(cleaned)
    }
}

#[cfg(windows)]
fn process_path_of(hwnd: windows::Win32::Foundation::HWND) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::GetModuleFileNameExW;

    unsafe {
        let handle = process_of(hwnd)?;
        let mut buf = [0u16; 260];
        let len = GetModuleFileNameExW(handle, None, &mut buf);
        let _ = CloseHandle(handle);
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Read a process's token-elevation flag. `process` must be a handle opened
/// with at least PROCESS_QUERY_LIMITED_INFORMATION. `Ok(_)` is a definitive
/// answer; `Err(())` means the token could not be opened or queried — for a
/// non-elevated caller inspecting an elevated target that failure is itself a
/// signal, so callers interpret it rather than defaulting it away.
#[cfg(windows)]
fn token_is_elevated(process: windows::Win32::Foundation::HANDLE) -> Result<bool, ()> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::OpenProcessToken;

    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(|_| ())?;
        let mut info = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let res = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut info as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        let _ = CloseHandle(token);
        res.map_err(|_| ())?;
        Ok(info.TokenIsElevated != 0)
    }
}

/// Whether KeepItLocal itself is running elevated. An elevated process can
/// inject input into windows at any integrity level, so this short-circuits
/// the (more expensive) target inspection in `target_blocked_by_elevation`.
#[cfg(windows)]
fn current_process_is_elevated() -> bool {
    use windows::Win32::System::Threading::GetCurrentProcess;
    // GetCurrentProcess hands back a pseudo-handle — never closed.
    token_is_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false)
}

/// Decide whether auto-paste into `hwnd`'s process would be silently dropped
/// by Windows UIPI. That happens when the target runs at a higher integrity
/// level (e.g. "Run as administrator") than a non-elevated KeepItLocal.
/// SendInput gives no error in that case — it *reports success* and the
/// keystroke vanishes — so it must be detected up front to explain it.
#[cfg(windows)]
fn target_blocked_by_elevation(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    // Elevated KeepItLocal can inject anywhere — no block is possible.
    if current_process_is_elevated() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return false;
    }
    let handle = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(h) => h,
        // Couldn't get even a limited handle — unknown, don't false-warn.
        Err(_) => return false,
    };
    let verdict = token_is_elevated(handle);
    unsafe {
        let _ = CloseHandle(handle);
    }
    match verdict {
        Ok(elevated) => elevated,
        // The handle opened but the token would not — the classic signature
        // of a non-elevated process looking at an elevated one. Treat as
        // blocked: a needless "press Ctrl+V" toast is far cheaper than a
        // keystroke that silently disappears.
        Err(()) => true,
    }
}

/// Poll until the OS reports `hwnd` as the foreground window, or give up
/// after ~300 ms. SetForegroundWindow returning success does not mean focus
/// has *settled* — there is a brief transitional window in which a SendInput
/// keystroke would land in the wrong place. Gating SendInput on this poll is
/// what removes the paste-into-the-wrong-app race.
#[cfg(windows)]
fn wait_until_foreground(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    for _ in 0..30 {
        if unsafe { GetForegroundWindow() } == hwnd {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    unsafe { GetForegroundWindow() == hwnd }
}

/// Bring `hwnd` to the foreground reliably. A plain SetForegroundWindow is
/// frequently refused by Win32 focus-stealing prevention; the robust fix is
/// to attach our input thread to the target window's thread for the duration
/// of the call, which makes Win32 treat the request as coming from the
/// foreground thread itself. Returns whether the target actually became the
/// foreground window (confirmed by `wait_until_foreground`, not by trusting
/// the SetForegroundWindow return value).
#[cfg(windows)]
pub(crate) fn focus_window_reliably(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowThreadProcessId, SetForegroundWindow,
    };

    // Fast path — our overlay just held focus, so we often still qualify to
    // hand it straight back without the attach-input dance.
    if unsafe { SetForegroundWindow(hwnd) }.as_bool() && wait_until_foreground(hwnd) {
        return true;
    }

    let target_thread = unsafe { GetWindowThreadProcessId(hwnd, None) };
    let our_thread = unsafe { GetCurrentThreadId() };
    if target_thread == 0 || target_thread == our_thread {
        return wait_until_foreground(hwnd);
    }

    unsafe {
        let attached = AttachThreadInput(our_thread, target_thread, true).as_bool();
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);
        let ok = wait_until_foreground(hwnd);
        if attached {
            let _ = AttachThreadInput(our_thread, target_thread, false);
        }
        ok
    }
}

/// Refocus a previously-captured HWND and inject Ctrl+V into it. Hardened
/// path: validate the handle is still a live window, detect an elevated
/// target that would silently swallow the keystroke, then restore focus
/// reliably (attach-input fallback) and only fire SendInput once the OS
/// confirms the target is foreground. Every failure mode emits a specific
/// fallback toast — the text is already on the clipboard, so the worst case
/// is always a manual Ctrl+V, never lost data.
#[cfg(windows)]
fn focus_and_paste(hwnd_value: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;

    let hwnd = HWND(hwnd_value);

    // The remembered window may have been closed while the overlay was open.
    if !unsafe { IsWindow(hwnd) }.as_bool() {
        emit_paste_fallback(
            "The previous window is gone — switch to an app and press Ctrl+V to paste.",
        );
        return;
    }

    // A non-elevated KeepItLocal cannot inject input into an elevated app:
    // UIPI drops the keystroke and SendInput still reports success. Detect it
    // up front so the user gets a real explanation instead of a no-op.
    if target_blocked_by_elevation(hwnd) {
        emit_paste_fallback(
            "The target app is running as administrator — auto-paste is blocked. Press Ctrl+V to paste manually.",
        );
        return;
    }

    if !focus_window_reliably(hwnd) {
        emit_paste_fallback(
            "Couldn't refocus the previous app — press Ctrl+V to paste manually.",
        );
        return;
    }

    // Synthesize the keystroke as four INPUT events: Ctrl down, V down, V up,
    // Ctrl up. This is the same sequence the OS dispatches for a real Ctrl+V
    // press, so the target window's keyboard handler sees an indistinguishable
    // event stream.
    fn make_key_event(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let inputs = [
        make_key_event(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        make_key_event(VK_V, KEYBD_EVENT_FLAGS(0)),
        make_key_event(VK_V, KEYEVENTF_KEYUP),
        make_key_event(VK_CONTROL, KEYEVENTF_KEYUP),
    ];

    unsafe {
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if (sent as usize) != inputs.len() {
            // SendInput was blocked or partially dispatched. Most common
            // cause: another process has a low-level keyboard hook with
            // higher privilege (some AV / parental control tools). The text
            // is still on the clipboard so the user can fall back to manual
            // paste; surface a toast so they know what happened.
            eprintln!(
                "clipboard_history: SendInput dispatched {sent}/{} events",
                inputs.len()
            );
            emit_paste_fallback(
                "Auto-paste was blocked — press Ctrl+V to paste manually.",
            );
        }
    }
}

/// Refocus a previously-captured HWND and type `text` into it as synthetic
/// Unicode keystrokes. This is the "type-out" dictation output mode: unlike
/// `focus_and_paste` it never touches the clipboard, so dictated text does
/// not clobber what the user had copied. Shares the exact same focus-restore
/// hardening as the paste path (live-window check, elevation-block detection,
/// reliable refocus). On any failure the text is rescued onto the clipboard
/// (history-suppressed) so it is never lost, and a specific fallback toast
/// explains what happened.
#[cfg(windows)]
fn focus_and_type(hwnd_value: isize, text: String) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_RETURN,
    };
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;

    let hwnd = HWND(hwnd_value);

    // The remembered window may have been closed since the key went down.
    if !unsafe { IsWindow(hwnd) }.as_bool() {
        rescue_text_to_clipboard(&text);
        emit_paste_fallback(
            "The previous window is gone — your dictation is on the clipboard, press Ctrl+V to paste.",
        );
        return;
    }

    // A non-elevated KeepItLocal cannot inject input into an elevated app:
    // UIPI drops the keystrokes and SendInput still reports success. Detect
    // it up front so the user gets a real explanation, not a silent no-op.
    if target_blocked_by_elevation(hwnd) {
        rescue_text_to_clipboard(&text);
        emit_paste_fallback(
            "The target app is running as administrator — typing is blocked. Your dictation is on the clipboard, press Ctrl+V.",
        );
        return;
    }

    if !focus_window_reliably(hwnd) {
        rescue_text_to_clipboard(&text);
        emit_paste_fallback(
            "Couldn't refocus the previous app — your dictation is on the clipboard, press Ctrl+V.",
        );
        return;
    }

    // Build the synthetic keystroke stream — a key-down + key-up pair per
    // character. Printable characters go in via KEYEVENTF_UNICODE (wVk = 0,
    // wScan = the UTF-16 code unit), so the target sees the literal
    // character regardless of its active keyboard layout; a non-BMP char is
    // simply its two surrogate UTF-16 units sent consecutively, which
    // Windows recombines. Newlines are the exception — a literal U+000A
    // does NOT produce an Enter press in most editors, so '\n' is sent as a
    // real VK_RETURN keystroke; a '\r' is dropped so CRLF yields one Enter.
    fn unicode_event(scan: u16, up: bool) -> INPUT {
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
                    dwExtraInfo: 0,
                },
            },
        }
    }
    fn vk_event(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        let flags = if up {
            KEYEVENTF_KEYUP
        } else {
            KEYBD_EVENT_FLAGS(0)
        };
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2);
    for ch in text.chars() {
        match ch {
            '\r' => continue,
            '\n' => {
                inputs.push(vk_event(VK_RETURN, false));
                inputs.push(vk_event(VK_RETURN, true));
            }
            _ => {
                let mut buf = [0u16; 2];
                for unit in ch.encode_utf16(&mut buf) {
                    inputs.push(unicode_event(*unit, false));
                    inputs.push(unicode_event(*unit, true));
                }
            }
        }
    }

    if inputs.is_empty() {
        return;
    }

    unsafe {
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if (sent as usize) != inputs.len() {
            // SendInput was blocked or partially dispatched — most often a
            // higher-privilege low-level keyboard hook (some AV / parental
            // tools). Rescue the full text so nothing is lost.
            eprintln!(
                "clipboard_history: type-out SendInput dispatched {sent}/{} events",
                inputs.len()
            );
            rescue_text_to_clipboard(&text);
            emit_paste_fallback(
                "Typing was blocked — your dictation is on the clipboard, press Ctrl+V to paste.",
            );
        }
    }
}

/// Look up a Win32 registered clipboard format ID by its MIME-typed name.
/// `RegisterClipboardFormatW` is idempotent — repeated calls with the same
/// name return the same ID for the process's lifetime, and 0 on failure.
/// We cache the result in a OnceLock so we don't re-do the wide-string
/// conversion on every clipboard event.
#[cfg(windows)]
fn registered_format(slot: &OnceLock<u32>, name: &str) -> u32 {
    *slot.get_or_init(|| {
        use windows::core::PCWSTR;
        use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) }
    })
}

/// Read raw bytes from the OS clipboard at the given format ID. Caller is
/// responsible for knowing what those bytes represent (PNG file, JPEG file,
/// CF_DIB blob, etc.). Returns None if the format isn't present on the
/// clipboard right now, the data is empty, or any of the Win32 calls fail.
///
/// Use this for the registered MIME-typed formats (image/gif, image/png,
/// image/jpeg, image/webp) where the data IS already the file format the
/// source app put on the clipboard. The dedicated CF_DIB path stays separate
/// because it needs the BMP-header-wrapping conversion dance.
#[cfg(windows)]
fn read_clipboard_raw_bytes(format_id: u32) -> Option<Vec<u8>> {
    use core::ffi::c_void;
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData};
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    if format_id == 0 {
        return None;
    }

    if !open_clipboard_with_retry() {
        return None;
    }

    let bytes = unsafe {
        let handle: HANDLE = match GetClipboardData(format_id) {
            Ok(h) => h,
            Err(_) => {
                let _ = CloseClipboard();
                return None;
            }
        };
        if handle.0 == 0 {
            let _ = CloseClipboard();
            return None;
        }
        let hglobal = HGLOBAL(handle.0 as *mut c_void);
        let ptr = GlobalLock(hglobal) as *const u8;
        if ptr.is_null() {
            let _ = CloseClipboard();
            return None;
        }
        let size = GlobalSize(hglobal);
        let copied = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(hglobal);
        copied
    };

    unsafe {
        let _ = CloseClipboard();
    }

    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}

/// Read CF_HDROP (file-paths-on-clipboard) and return a list of paths.
/// Empty Vec if no files / read failed. Uses `DragQueryFileW` from the
/// shell crate which handles the DROPFILES struct unpacking for us — much
/// safer than walking the bytes by hand.
///
/// This is our workaround for "right-click → Copy image" on a GIF in
/// Chrome (which strips the animation): if the user instead saves the .gif
/// and copies the file from Explorer, the path lands here, we read the
/// actual bytes off disk, and store them with format = "gif" so animation
/// survives the round trip.
#[cfg(windows)]
fn read_clipboard_file_paths() -> Vec<PathBuf> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData};
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

    if !open_clipboard_with_retry() {
        return Vec::new();
    }

    let paths = unsafe {
        let handle: HANDLE = match GetClipboardData(CF_HDROP) {
            Ok(h) => h,
            Err(_) => {
                let _ = CloseClipboard();
                return Vec::new();
            }
        };
        if handle.0 == 0 {
            let _ = CloseClipboard();
            return Vec::new();
        }

        // HDROP wraps the same HANDLE the clipboard gave us. DragQueryFileW
        // with index 0xFFFFFFFF returns the count of files; calling it again
        // with each index returns the path.
        let hdrop = HDROP(handle.0);
        let count = DragQueryFileW(hdrop, 0xFFFF_FFFFu32, None);
        let mut out: Vec<PathBuf> = Vec::with_capacity(count as usize);
        for i in 0..count {
            // Win32 path length is bounded by MAX_PATH (260) on legacy APIs
            // but newer Windows allows longer with the long-path manifest.
            // We size to 512 wchars for safety; truncation here is preferred
            // over allocation gymnastics on the hot path.
            let mut buf: [u16; 512] = [0; 512];
            let written = DragQueryFileW(hdrop, i, Some(&mut buf));
            if written == 0 {
                continue;
            }
            let path = String::from_utf16_lossy(&buf[..written as usize]);
            out.push(PathBuf::from(path));
        }
        out
    };

    unsafe {
        let _ = CloseClipboard();
    }
    paths
}

/// Read CF_DIB from the OS clipboard, convert to PNG bytes via the `image`
/// crate. Returns None if the clipboard doesn't have image data or if
/// decoding fails (malformed DIB, unsupported bit depth, etc).
///
/// Conversion trick: the clipboard's CF_DIB blob is exactly the body of a
/// BMP file minus the 14-byte BITMAPFILEHEADER. We prepend a synthetic
/// header so `image::load_from_memory` can decode it as BMP, then re-encode
/// to PNG. Round-trip is lossless for 24/32-bit images (the common cases).
#[cfg(windows)]
fn read_clipboard_image_as_png() -> Option<Vec<u8>> {
    use core::ffi::c_void;
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData};
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    if !open_clipboard_with_retry() {
        return None;
    }

    let dib_bytes = unsafe {
        let handle: HANDLE = match GetClipboardData(CF_DIB) {
            Ok(h) => h,
            Err(_) => {
                let _ = CloseClipboard();
                return None;
            }
        };
        if handle.0 == 0 {
            let _ = CloseClipboard();
            return None;
        }
        let hglobal = HGLOBAL(handle.0 as *mut c_void);
        let ptr = GlobalLock(hglobal) as *const u8;
        if ptr.is_null() {
            let _ = CloseClipboard();
            return None;
        }
        let size = GlobalSize(hglobal);
        // Copy the DIB out of the locked global before unlocking — we'll
        // process it after the clipboard is closed.
        let copied = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(hglobal);
        copied
    };

    unsafe {
        let _ = CloseClipboard();
    }

    if dib_bytes.is_empty() {
        return None;
    }

    // Wrap the DIB in a synthetic BMP file so the image crate can decode it.
    // BITMAPFILEHEADER = 14 bytes: signature "BM", file size (u32), 2x u16
    // reserved, pixel data offset (u32). The image crate uses the offset to
    // find pixel data — we set it correctly by inspecting the DIB header
    // size at offset 0 of the DIB.
    let dib_header_size = u32::from_le_bytes([
        *dib_bytes.first().unwrap_or(&0),
        *dib_bytes.get(1).unwrap_or(&0),
        *dib_bytes.get(2).unwrap_or(&0),
        *dib_bytes.get(3).unwrap_or(&0),
    ]) as usize;
    if dib_header_size < 12 || dib_header_size > dib_bytes.len() {
        eprintln!("clipboard_history: implausible DIB header size {dib_header_size}");
        return None;
    }
    // For 8-bit and below there's a color table after the header — we'd
    // need to count entries. Modern screenshot tools all use 24/32-bit, so
    // for v1 we trust the simple "pixels start right after the header" path.
    // If image::load fails we'll bail out gracefully.
    let pixel_offset = (14 + dib_header_size) as u32;
    let total_size = (14 + dib_bytes.len()) as u32;

    let mut bmp = Vec::with_capacity(14 + dib_bytes.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&total_size.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes()); // reserved
    bmp.extend_from_slice(&pixel_offset.to_le_bytes());
    bmp.extend_from_slice(&dib_bytes);

    let img = match image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp) {
        Ok(i) => i,
        Err(error) => {
            eprintln!("clipboard_history: BMP decode failed: {error}");
            return None;
        }
    };

    let mut png_buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png_buf);
    if let Err(error) = img.write_to(&mut cursor, image::ImageFormat::Png) {
        eprintln!("clipboard_history: PNG encode failed: {error}");
        return None;
    }
    Some(png_buf)
}

/// Write an image (read from disk) back to the OS clipboard in *multiple*
/// formats simultaneously: the original MIME registered format (so
/// animation/quality survive) AND CF_DIB (so apps that only read the
/// standard format still get a usable bitmap, even if it's only the first
/// frame for animated images).
///
/// Format matrix:
///   "gif"  → image/gif (animated) + CF_DIB (first frame)
///   "png"  → image/png (lossless) + CF_DIB
///   "jpeg" → image/jpeg (preserves quality) + CF_DIB
///   "webp" → image/webp (animated for WebP-anim) + CF_DIB
///   "bmp" or None → CF_DIB only (our PNG-from-CF_DIB fallback path)
///
/// The CF_DIB part requires re-decoding the image and re-encoding as BMP.
/// For static images this is fast and lossless. For animated formats, the
/// decode produces the FIRST FRAME — which is what Paint/Word want anyway
/// (they don't support animation in pastes).
#[cfg(windows)]
fn write_image_to_clipboard(
    image_path: &std::path::Path,
    image_format: Option<&str>,
) -> Result<(), String> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, SetClipboardData};
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

    // Step 1: read the raw image bytes off disk. For registered formats
    // these go back to the clipboard verbatim, preserving the original
    // file's contents bit-for-bit (animation, quality, color profiles, etc).
    let raw_bytes = std::fs::read(image_path).map_err(|e| format!("read image: {e}"))?;

    // Step 2: decode + re-encode as BMP so we can also publish CF_DIB. This
    // path is what makes the image paste cleanly into apps like Paint, MS
    // Word, Photoshop, etc., that don't read MIME-typed registered formats.
    let img = image::open(image_path).map_err(|e| format!("decode image: {e}"))?;
    let mut bmp_buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bmp_buf);
    img.write_to(&mut cursor, image::ImageFormat::Bmp)
        .map_err(|e| format!("encode bmp: {e}"))?;
    if bmp_buf.len() < 14 {
        return Err("BMP encoding too short".to_string());
    }
    let dib_bytes = bmp_buf[14..].to_vec();

    // Step 3: figure out which registered format to also publish, if any.
    // The CF_DIB write happens for every image; the registered format write
    // is conditional on the source format we captured.
    let registered_id: Option<u32> = match image_format {
        Some("gif") => Some(registered_format(&FORMAT_GIF, "image/gif")),
        Some("png") => Some(registered_format(&FORMAT_PNG, "image/png")),
        Some("jpeg") => Some(registered_format(&FORMAT_JPEG, "image/jpeg")),
        Some("webp") => Some(registered_format(&FORMAT_WEBP, "image/webp")),
        // "bmp" / None / unknown → CF_DIB only.
        _ => None,
    };

    /// Inner helper: copy a byte slice into a GMEM_MOVEABLE handle suitable
    /// for SetClipboardData. Returns the handle as a Win32 HANDLE. Caller
    /// retains ownership until SetClipboardData succeeds — at which point
    /// the OS takes over and frees on EmptyClipboard / process exit.
    unsafe fn copy_to_global(bytes: &[u8]) -> Result<HANDLE, String> {
        let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes.len())
            .map_err(|e| format!("GlobalAlloc failed: {e}"))?;
        if hglobal.0.is_null() {
            return Err("GlobalAlloc returned null".to_string());
        }
        let ptr = GlobalLock(hglobal) as *mut u8;
        if ptr.is_null() {
            return Err("GlobalLock failed".to_string());
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(hglobal);
        Ok(HANDLE(hglobal.0 as isize))
    }

    if !open_clipboard_with_retry() {
        return Err("could not open clipboard (held by another app)".to_string());
    }
    unsafe {
        if EmptyClipboard().is_err() {
            let _ = CloseClipboard();
            return Err("EmptyClipboard failed".to_string());
        }

        // Write CF_DIB first — it's the lowest-common-denominator format and
        // must always succeed for the paste to work in at-minimum apps like
        // Paint. Failure here is fatal.
        let dib_handle = copy_to_global(&dib_bytes).map_err(|e| {
            let _ = CloseClipboard();
            e
        })?;
        if SetClipboardData(CF_DIB, dib_handle).is_err() {
            let _ = CloseClipboard();
            return Err("SetClipboardData(CF_DIB) failed".to_string());
        }

        // Optionally also publish the registered MIME format so animated/
        // higher-fidelity pastes work in browsers, Discord, etc. Failure
        // here is non-fatal: the CF_DIB write already succeeded, so paste
        // still works — animation just doesn't survive.
        if let Some(fmt_id) = registered_id {
            if fmt_id != 0 {
                match copy_to_global(&raw_bytes) {
                    Ok(handle) => {
                        if SetClipboardData(fmt_id, handle).is_err() {
                            eprintln!(
                                "clipboard_history: SetClipboardData for registered format {} failed; paste will fall back to CF_DIB",
                                fmt_id
                            );
                        }
                    }
                    Err(error) => {
                        eprintln!(
                            "clipboard_history: alloc for registered format failed ({error}); paste will fall back to CF_DIB"
                        );
                    }
                }
            }
        }

        let _ = CloseClipboard();
    }
    Ok(())
}

#[cfg(not(windows))]
fn read_clipboard_image_as_png() -> Option<Vec<u8>> {
    None
}

#[cfg(not(windows))]
fn write_image_to_clipboard(
    _image_path: &std::path::Path,
    _image_format: Option<&str>,
) -> Result<(), String> {
    Err("Clipboard image write is Windows-only for now".to_string())
}

#[cfg(not(windows))]
fn write_text_to_clipboard(_text: &str) -> Result<(), String> {
    Err("Clipboard write is Windows-only for now".to_string())
}

#[cfg(not(windows))]
fn write_text_to_clipboard_marked(_text: &str, _quiet: bool) -> Result<(), String> {
    Err("Clipboard write is Windows-only for now".to_string())
}

#[cfg(not(windows))]
fn get_foreground_process_name() -> Option<String> {
    None
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Copying >256KB of non-ASCII text used to be able to kill the process:
    /// `String::truncate` panics on a non-char-boundary index, and the caller
    /// is a Win32 window proc where unwinding is UB. Georgian/Cyrillic/CJK/
    /// emoji are all multi-byte, so this is ordinary text, not an edge case.
    #[test]
    fn truncating_multibyte_text_never_splits_a_char() {
        // 'ა' (Georgian an) is 3 bytes — 3 does not divide the cut points
        // below, so a naive truncate lands mid-character for most of them.
        let s = "ა".repeat(4096);
        assert_eq!(s.len(), 12_288);

        for max in [1, 2, 3, 4, 5, 100, 1001, 12_287] {
            let end = floor_char_boundary(&s, max);
            assert!(end <= max, "must not exceed the cap");
            assert!(s.is_char_boundary(end), "must land on a boundary");
            // The real call: this is the line that panicked before the fix.
            let mut t = s.clone();
            t.truncate(end);
            assert!(t.chars().all(|c| c == 'ა'), "no mangled char survived");
        }

        // A cap at/above the length is a no-op, not a truncation.
        assert_eq!(floor_char_boundary(&s, s.len()), s.len());
        assert_eq!(floor_char_boundary(&s, s.len() + 999), s.len());
        // ASCII: every index is a boundary, so the cap is returned verbatim —
        // this is why the bug hid for so long.
        assert_eq!(floor_char_boundary("hello world", 7), 7);
    }

    /// The Snipping Tool bug: one snip fired several WM_CLIPBOARDUPDATE and
    /// landed as duplicate entries. Same bytes back-to-back = one copy.
    /// Different bytes must always pass, or we'd swallow real copies.
    /// (Serialized against the other LAST_IMAGE test — they share a global.)
    #[test]
    fn image_burst_drops_only_immediate_identical_repeats() {
        let _g = image_test_lock();
        reset_last_image();

        let snip = b"pretend-png-bytes";
        // First sight of a copy is never a duplicate...
        assert!(!is_duplicate_image_burst(snip));
        // ...but the echo of that same copy is.
        assert!(is_duplicate_image_burst(snip));
        assert!(is_duplicate_image_burst(snip));

        // A genuinely different image must still get through, even instantly.
        assert!(!is_duplicate_image_burst(b"a-different-image"));
        // ...and now IT is the one being echo-suppressed, not the old snip.
        assert!(is_duplicate_image_burst(b"a-different-image"));
        assert!(!is_duplicate_image_burst(snip));
    }

    /// Outside the window the same image is an intentional re-copy and must
    /// be kept — this is what stops the guard from becoming silent data loss.
    #[test]
    fn image_burst_keeps_deliberate_recopy_after_window() {
        let _g = image_test_lock();
        reset_last_image();

        let img = b"same-image-twice";
        assert!(!is_duplicate_image_burst(img));

        // Backdate the recorded sighting to just outside the burst window,
        // rather than sleeping for two real seconds in a unit test.
        {
            let mut guard = last_image()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some((_, ts)) = guard.as_mut() {
                *ts -= IMAGE_BURST_WINDOW_MS + 1;
            }
        }

        assert!(!is_duplicate_image_burst(img));
    }

    fn reset_last_image() {
        *last_image()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    /// LAST_IMAGE is process-global, so the two tests above would race under
    /// cargo's default threaded runner.
    fn image_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn categorizes_url() {
        assert_eq!(categorize("https://github.com/foo"), Category::Url);
        assert_eq!(categorize("http://localhost:3000"), Category::Url);
        assert_eq!(categorize("ftp://files.example.com"), Category::Url);
    }

    #[test]
    fn categorizes_email() {
        assert_eq!(categorize("alice@example.com"), Category::Email);
        assert_eq!(categorize("bob.smith+tag@sub.example.co"), Category::Email);
        // Has whitespace → not an email.
        assert_eq!(categorize("send to alice@example.com"), Category::Text);
        // No TLD → not an email.
        assert_eq!(categorize("alice@localhost"), Category::Text);
    }

    #[test]
    fn categorizes_json() {
        assert_eq!(categorize(r#"{"name":"Alice"}"#), Category::Json);
        assert_eq!(categorize(r#"[1, 2, 3]"#), Category::Json);
        // Has braces but isn't really JSON-shaped → falls back.
        assert_eq!(categorize("{not really json}"), Category::Text);
    }

    #[test]
    fn categorizes_color() {
        assert_eq!(categorize("#fff"), Category::Color);
        assert_eq!(categorize("#FF00AA"), Category::Color);
        assert_eq!(categorize("rgb(255, 0, 128)"), Category::Color);
        assert_eq!(categorize("hsl(120, 50%, 50%)"), Category::Color);
        assert_eq!(categorize("#xyzxyz"), Category::Text); // bad hex
    }

    #[test]
    fn categorizes_file_path() {
        assert_eq!(categorize("C:\\Users\\Alice\\Documents"), Category::FilePath);
        assert_eq!(categorize("D:/projects/foo"), Category::FilePath);
        assert_eq!(categorize("/usr/local/bin/git"), Category::FilePath);
        assert_eq!(
            categorize("\\\\server\\share\\file.txt"),
            Category::FilePath
        );
    }

    #[test]
    fn categorizes_hash() {
        // MD5 (32 hex chars).
        assert_eq!(
            categorize("d41d8cd98f00b204e9800998ecf8427e"),
            Category::Hash
        );
        // SHA-1 (40 hex chars).
        assert_eq!(
            categorize("da39a3ee5e6b4b0d3255bfef95601890afd80709"),
            Category::Hash
        );
        // UUID with dashes.
        assert_eq!(
            categorize("550e8400-e29b-41d4-a716-446655440000"),
            Category::Hash
        );
        // Short string → not a hash.
        assert_eq!(categorize("abc123"), Category::Text);
    }

    #[test]
    fn categorizes_number() {
        assert_eq!(categorize("42"), Category::Number);
        assert_eq!(categorize("-3.14"), Category::Number);
        assert_eq!(categorize("123456"), Category::Number); // OTP-shaped
        assert_eq!(categorize("12a"), Category::Text); // not purely numeric
    }

    #[test]
    fn categorizes_code() {
        let snippet = "fn main() {\n    println!(\"hello\");\n}";
        assert_eq!(categorize(snippet), Category::Code);
        let multiline_text = "Hello\nWorld";
        // Multi-line but no code signals → plain text.
        assert_eq!(categorize(multiline_text), Category::Text);
    }

    #[test]
    fn empty_categorizes_as_text() {
        assert_eq!(categorize(""), Category::Text);
        assert_eq!(categorize("   "), Category::Text);
    }

    #[test]
    fn default_exclusions_lowercase_known_managers() {
        // Sanity check that the default list contains entries — change-detector
        // for the constant.
        assert!(DEFAULT_EXCLUSIONS.iter().any(|s| s == &"KeePassXC"));
        assert!(DEFAULT_EXCLUSIONS.iter().any(|s| s == &"1Password"));
        assert!(DEFAULT_EXCLUSIONS.iter().any(|s| s == &"Bitwarden"));
    }

    // ─── #11 — retention / exclusion / at-rest round-trip ────────────────

    /// Minimal text entry for retention tests; cases tweak fields directly.
    fn test_text_entry(captured_at_ms: i64) -> ClipboardEntry {
        ClipboardEntry {
            id: 1,
            captured_at_ms,
            kind: EntryKind::Text,
            text: String::new(),
            source_app: None,
            source_app_path: None,
            sensitive_kinds: Vec::new(),
            category: Category::Text,
            is_pinned: false,
            pin_label: None,
            image_path: None,
            thumbnail_path: None,
            image_width: None,
            image_height: None,
            image_size_bytes: None,
            image_format: None,
        }
    }

    #[test]
    fn retention_keeps_recent_entry_expires_old() {
        // cutoff = 500: an entry at 1000 is newer (kept); at 100 is older (expired).
        assert!(entry_survives_retention(&test_text_entry(1_000), Some(500), Some(500), 0));
        assert!(!entry_survives_retention(&test_text_entry(100), Some(500), Some(500), 0));
    }

    #[test]
    fn retention_disabled_keeps_old_entry() {
        // text_cutoff None = time-based retention disabled → ancient entry kept.
        assert!(entry_survives_retention(&test_text_entry(1), None, Some(500), 0));
    }

    #[test]
    fn retention_pinned_entry_is_immune() {
        let mut e = test_text_entry(1);
        e.is_pinned = true;
        e.sensitive_kinds = vec!["credit_card".to_string()];
        // Ancient AND sensitive — but pinned beats every cutoff.
        assert!(entry_survives_retention(&e, Some(1_000_000), Some(1_000_000), 999_999));
    }

    #[test]
    fn retention_text_and_image_use_separate_cutoffs() {
        let text = test_text_entry(300);
        let mut image = test_text_entry(300);
        image.kind = EntryKind::Image;
        // text_cutoff 200 → text (300) kept; image_cutoff 400 → image (300) expired.
        assert!(entry_survives_retention(&text, Some(200), Some(400), 0));
        assert!(!entry_survives_retention(&image, Some(200), Some(400), 0));
    }

    #[test]
    fn retention_sensitive_entry_expires_on_short_window() {
        let mut e = test_text_entry(1_000);
        e.sensitive_kinds = vec!["github_token".to_string()];
        // Newer than the text cutoff, but older than the sensitive cutoff →
        // expired anyway (a copied credential must not linger).
        assert!(!entry_survives_retention(&e, Some(0), Some(0), 2_000));
        // Sensitive cutoff in the past → still inside the window → kept.
        assert!(entry_survives_retention(&e, Some(0), Some(0), 500));
    }

    #[test]
    fn exclusion_matches_case_insensitively() {
        let list = vec!["keepassxc".to_string(), "1password".to_string()];
        assert!(app_matches_exclusion(Some("KeePassXC"), &list));
        assert!(app_matches_exclusion(Some("keepassxc"), &list));
        assert!(app_matches_exclusion(Some("1Password"), &list));
    }

    #[test]
    fn exclusion_rejects_unlisted_and_none() {
        let list = vec!["keepassxc".to_string()];
        assert!(!app_matches_exclusion(Some("chrome"), &list));
        assert!(!app_matches_exclusion(None, &list));
        assert!(!app_matches_exclusion(Some("keepassxc"), &[]));
    }

    #[cfg(windows)]
    #[test]
    fn at_rest_encryption_round_trips() {
        // protect_at_rest → unprotect_at_rest must recover the exact bytes,
        // and the protected blob must not equal the plaintext (it is
        // DPAPI-wrapped). Assumes a real Windows user context, as `cargo test`
        // always has.
        let plain = br#"{"entries":[],"retentionDays":14}"#.to_vec();
        let blob = protect_at_rest(&plain).expect("DPAPI works for this user");
        assert_ne!(blob, plain, "protect_at_rest should DPAPI-encrypt, not pass plaintext through");
        assert_eq!(unprotect_at_rest(&blob), plain);
        // An empty payload round-trips too.
        assert_eq!(unprotect_at_rest(&protect_at_rest(b"").unwrap()), b"".to_vec());
    }

    // ─── Search: capture decisions taken when the copy is announced ──────

    #[test]
    fn self_write_is_ours_only_until_its_deadline() {
        let mut until = 1_500;
        assert!(take_self_write(&mut until, 1_200), "an update inside the window is our own write");
        assert_eq!(until, 0, "and it's consumed");
        assert!(!take_self_write(&mut until, 1_300), "the next update is the user's");

        // A write whose update never came doesn't swallow a copy minutes later.
        let mut stale = 1_500;
        assert!(!take_self_write(&mut stale, 60_000));
        assert_eq!(stale, 0);

        let mut none = 0;
        assert!(!take_self_write(&mut none, 0));
    }

    #[test]
    fn settle_gate_arms_once_per_window_and_survives_a_new_window() {
        let gate = SettleGate::new();
        assert!(gate.arm(10), "first update arms a timer");
        assert!(!gate.arm(10), "a burst behind it doesn't");
        gate.done(10);
        assert!(gate.arm(10), "after the timer fired, the next update arms again");

        // The listener thread died with its timer armed; the rebuilt window
        // (a new hwnd) must still get timers.
        assert!(gate.arm(20), "a new window isn't blocked by the old one's timer");
        gate.done(10);
        assert!(!gate.arm(20), "the old window's late 'done' doesn't disarm the new one");
        gate.reset();
        assert!(gate.arm(20), "reset on (re)creation clears it");
    }

    #[test]
    fn history_marker_zero_or_unreadable_forbids_capture() {
        assert!(!history_marker_forbids(false, None));
        assert!(history_marker_forbids(true, Some(&[0, 0, 0, 0])));
        assert!(!history_marker_forbids(true, Some(&[1, 0, 0, 0])));
        assert!(history_marker_forbids(true, None), "present but unreadable: not kept");
        assert!(history_marker_forbids(true, Some(&[0])), "short: not kept");
    }

    #[test]
    fn snapshot_decides_capture() {
        let exclusions = vec!["keepassxc".to_string()];
        let plain = CopySnapshot { source_app: Some("notepad".into()), ..Default::default() };
        assert!(snapshot_allows_capture(&plain, &exclusions, None));

        let ours = CopySnapshot { ours: true, ..plain.clone() };
        assert!(!snapshot_allows_capture(&ours, &exclusions, None));

        let marked = CopySnapshot { excluded: true, ..plain.clone() };
        assert!(!snapshot_allows_capture(&marked, &exclusions, None));

        // Taken from the app that owned the clipboard when it was announced,
        // whatever has the foreground by the time the data is read.
        let manager = CopySnapshot { source_app: Some("KeePassXC".into()), ..Default::default() };
        assert!(!snapshot_allows_capture(&manager, &exclusions, None));

        let no_history = CopySnapshot { history_marker: true, ..plain.clone() };
        assert!(!snapshot_allows_capture(&no_history, &exclusions, Some(&0u32.to_le_bytes())));
        assert!(snapshot_allows_capture(&no_history, &exclusions, Some(&1u32.to_le_bytes())));
    }

    #[test]
    fn a_failed_encrypt_skips_the_save_instead_of_writing_plaintext() {
        let plain = b"hunter2 is not a real secret";
        assert_eq!(seal_or_skip(plain, |_| Err("no DPAPI".to_string())), None);
        assert_eq!(seal_or_skip(plain, |b| Ok(b.iter().rev().copied().collect())), Some(plain.iter().rev().copied().collect()));
    }
}
