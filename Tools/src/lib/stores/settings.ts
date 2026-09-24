import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { get, writable } from 'svelte/store';
import { toast } from '$lib/stores/toasts';

/** Runtime-only voice-model download state. Kept separate from persisted
 * settings so an in-flight download remains visible after Settings remounts. */
export type VoiceModelDownloadProgress = { downloaded: number; total: number };
export const settingsVoiceModelDownloadBusy = writable<Record<string, boolean>>({});
export const settingsVoiceModelDownloadProgress = writable<Record<string, VoiceModelDownloadProgress>>({});

/** Runtime-only Full U²-Net download state. It stays outside persisted
 * settings so Settings can remount without losing an active download view. */
export type BackgroundRemovalModelDownloadProgress = { downloaded: number; total: number };
export type BackgroundRemovalModelStatus = {
    available: boolean;
    path: string | null;
    sizeBytes: number;
};
export const settingsBackgroundRemovalModelDownloadBusy = writable(false);
export const settingsBackgroundRemovalModelDownloadProgress = writable<BackgroundRemovalModelDownloadProgress | null>(null);
export const settingsBackgroundRemovalModelStatus = writable<BackgroundRemovalModelStatus | null>(null);

/** Pretty-print a hotkey config string for user-facing error messages.
 * `CommandOrControl+Shift+V` reads as `Ctrl + Shift + V`. */
function prettyShortcut(value: string | undefined): string {
    return (value ?? '').replace(/CommandOrControl/gi, 'Ctrl').replace(/\+/g, ' + ');
}

/** All themes ship with the same semantic token contract — switching is a
 * pure CSS variable swap (no per-component overrides). When adding a new
 * theme, define its `:root[data-theme='<id>']` block in `styles.css` and
 * add the id here. */
export type Theme =
    | 'dark'
    | 'light'
    | 'midnight'    // Pure-black for OLED — Palette Appearance Wave B (2026-05-27)
    | 'sepia'       // Warm reading tones — Palette Appearance Wave B (2026-05-27)
    | 'dracula'
    | 'nord'
    | 'tokyo-night'; // Deep blues + violets — Palette Appearance Wave B (2026-05-27)

/** The UI ships in **English only**. The `Locale` type still carries `'ka'`
 * because the VOICE/dictation engine is locale-aware (Vosk Georgian model →
 * `voiceModelLocale()` → Georgian command grammar) and Georgian OCR is still
 * supported — only the UI *translation* layer + language picker were removed.
 * `LOCALE_OPTIONS` therefore lists English alone (it drives the UI, not voice). */
export type Locale = 'en' | 'ka';

export const LOCALE_OPTIONS: Array<{ id: Locale; label: string; nativeLabel: string }> = [
    { id: 'en', label: 'English', nativeLabel: 'English' },
];

const VALID_LOCALES = new Set<Locale>(LOCALE_OPTIONS.map((l) => l.id));

/** Display metadata for each theme, used by the picker UI. The order here
 * is the order users see in the picker. */
export const THEME_OPTIONS: Array<{
    id: Theme;
    label: string;
    description: string;
    /** Three representative colors for the picker preview chip:
     * [background, surface/panel, accent]. */
    swatch: [string, string, string];
}> = [
    {
        id: 'dark',
        label: 'Dark',
        description: 'The default. Pure neutrals with the brand crimson accent.',
        swatch: ['#0a0a0a', '#1c1c1c', '#b5352c'],
    },
    {
        id: 'light',
        label: 'Light',
        description: 'Warm-neutral surfaces with a Linear-style indigo accent.',
        swatch: ['#f4f5f7', '#fbfcfd', '#5e6ad2'],
    },
    {
        id: 'dracula',
        label: 'Dracula',
        description: 'The classic editor palette. Pink accent on indigo.',
        swatch: ['#282a36', '#44475a', '#ff79c6'],
    },
    {
        id: 'nord',
        label: 'Nord',
        description: 'Cool arctic blues, easy on the eyes.',
        swatch: ['#2e3440', '#434c5e', '#88c0d0'],
    },
    // Palette Appearance Wave B (2026-05-27) — three additions to widen
    // the visual range. Each is implemented as a `:root[data-theme='<id>']`
    // block in styles.css; nothing else changes app-wide.
    {
        id: 'midnight',
        label: 'Midnight',
        description: 'Pure-black backgrounds. Maximum contrast, easy on OLED panels.',
        swatch: ['#000000', '#0a0a0c', '#b5352c'],
    },
    {
        id: 'sepia',
        label: 'Sepia',
        description: 'Warm, paper-like tones. Long-reading comfort.',
        swatch: ['#f5ecd9', '#fffaf0', '#a16207'],
    },
    {
        id: 'tokyo-night',
        label: 'Tokyo Night',
        description: 'Deep blues with violet accents. A community favorite.',
        swatch: ['#1a1b26', '#24283b', '#bb9af7'],
    },
];

const VALID_THEMES = new Set<Theme>(THEME_OPTIONS.map((t) => t.id));

/** UI font choices. The picker swaps the body/UI typeface only — code stays
 * on JetBrains Mono regardless. `'inter'` is the bundled default; `'system'`
 * is whatever Segoe UI the OS provides; the rest are bundled woff2 families.
 * When adding a font: add the id here, add a `FONT_OPTIONS` entry, bundle the
 * woff2 + `@font-face` in styles.css, and add a `:root[data-font='<id>']`
 * override block. */
export type UiFont = 'inter' | 'system' | 'geist' | 'ibm-plex' | 'atkinson' | 'serif';

/** Display metadata + the CSS font-family stack for each UI font. The `stack`
 * is the Latin face(s) only — `applyFont` / the CSS never need the Georgian
 * face here because `:root[data-font]` blocks in styles.css append
 * 'Noto Sans Georgian' themselves. Order = picker order. */
export const FONT_OPTIONS: Array<{
    id: UiFont;
    label: string;
    description: string;
    /** Font-family value used purely for the live preview in the picker card. */
    stack: string;
}> = [
    {
        id: 'inter',
        label: 'Inter',
        description: 'The default. Clean, neutral, highly legible at small sizes.',
        stack: "'Inter', system-ui, sans-serif",
    },
    {
        id: 'system',
        label: 'Segoe UI',
        description: "Windows' own UI font — feels native, costs nothing to load.",
        stack: "'Segoe UI', system-ui, sans-serif",
    },
    {
        id: 'geist',
        label: 'Geist',
        description: 'Modern geometric sans (Vercel). Crisp and product-like.',
        stack: "'Geist', system-ui, sans-serif",
    },
    {
        id: 'ibm-plex',
        label: 'IBM Plex Sans',
        description: 'Warm humanist sans with a touch of character.',
        stack: "'IBM Plex Sans', system-ui, sans-serif",
    },
    {
        id: 'atkinson',
        label: 'Atkinson Hyperlegible',
        description: 'Designed by the Braille Institute for maximum legibility.',
        stack: "'Atkinson Hyperlegible', system-ui, sans-serif",
    },
    {
        id: 'serif',
        label: 'Serif',
        description: 'Classic serif — system Georgia for Latin, Noto Serif for Georgian.',
        stack: "'Georgia', 'Noto Serif Georgian', serif",
    },
];

const VALID_FONTS = new Set<UiFont>(FONT_OPTIONS.map((f) => f.id));

/** One per-app hotkey row. `target` is a launcher path exactly as the app
 *  index stores it — an `.exe`, a Start-Menu `.lnk`, or a
 *  `shell:AppsFolder\<AUMID>` handle — so the backend can both match its
 *  window and launch it with the existing launcher plumbing. */
export interface AppHotkeyBinding {
    /** Stable row id, generated when the row is added. */
    id: string;
    /** Tauri-style chord, e.g. `CommandOrControl+Alt+1`. */
    shortcut: string;
    /** Launcher path of the bound app. */
    target: string;
    /** Display name shown in the Settings row. */
    name: string;
    /** Per-row switch, so a binding can be parked without deleting it. */
    enabled: boolean;
}

export interface AppSettings {
    theme: Theme;
    /** UI font — body/interface typeface. Code blocks stay monospaced
     * regardless. See UiFont / FONT_OPTIONS above. */
    uiFont: UiFont;
    /** UI language. Defaults to the first launch's detected OS locale if it's
     * one of our supported languages, otherwise English. Can always be
     * overridden in Settings → General → Language. */
    locale: Locale;
    /** The user's display name. Powers the Home greeting ("Good morning,
     *  {name}") and the {{name}} snippet placeholder. Empty until the user
     *  sets it in Settings → System. Local-only — never leaves the machine. */
    userName: string;
    sidebarCollapsed: boolean;
    overlayHotkeyMode: 'shortcut' | 'doubleSpace';
    overlayHotkeyShortcut: string;
    overlayDoubleSpaceIntervalMs: number;
    /** Whether the unified command-palette global hotkey is registered.
     *  Default on — the palette is the primary launcher. */
    commandOverlayEnabled: boolean;
    /** The Tauri-style global shortcut to summon the command palette.
     *  Default `Ctrl+Alt+K` (mirrors the Rust DEFAULT_COMMAND_OVERLAY_SHORTCUT). */
    commandOverlayShortcut: string;
    /** Whether the sticky quick-note global hotkey is registered. Default on
     *  — summoning a sticky note from anywhere is low-risk. */
    quickNoteHotkeyEnabled: boolean;
    /** The Tauri-style global shortcut that summons a sticky quick-note.
     *  Default `Ctrl+Alt+N` (mirrors the Rust DEFAULT_QUICK_NOTE_SHORTCUT). */
    quickNoteHotkeyShortcut: string;
    /** Per-app hotkeys — a user-built list (Settings → Shortcuts). Each chord
     *  toggles its app's window, or launches the app when it has none.
     *  Empty by default: every entry is one the user added by hand. */
    appHotkeys: AppHotkeyBinding[];
    /** Whether the screen-recording start/stop global hotkey is registered.
     *  Default on. */
    screenRecordingHotkeyEnabled: boolean;
    /** The Tauri-style global shortcut to start/stop a screen recording.
     *  Default `Ctrl+Alt+R`. */
    screenRecordingHotkeyShortcut: string;
    /** When true, bang prefixes like `g foo`, `?foo`, `gh foo` show a
     * web-search row in the overlay. Off by default — KeepItLocal is local-first. */
    webSearchEnabled: boolean;
    /** Master switch for bookmark + history search in the palette's Web chip.
     *  Off by default — reading browser profiles is surprising. On its own it
     *  surfaces BOOKMARKS only. */
    browserSearchEnabled: boolean;
    /** Additionally include browsing HISTORY. Off by default, separately —
     *  history is far more sensitive than bookmarks. */
    browserHistoryEnabled: boolean;
    /** When true, the dedicated clipboard-history overlay is reachable via
     * its own global hotkey. Default on — recall is a core productivity hit. */
    clipboardOverlayEnabled: boolean;
    /** The Tauri-style global shortcut to summon the clipboard overlay. */
    clipboardOverlayShortcut: string;
    /** When true, pressing Enter on a clipboard entry automatically pastes
     * into the previous app (synthetic Ctrl+V via SendInput). When false,
     * the entry is only put back on the clipboard and the user pastes
     * manually with Ctrl+V. Default on — this is the "magic" UX. */
    clipboardAutoPaste: boolean;
    /** Opt-in global snippet auto-expand (text-expander). Default OFF —
     *  it installs a keystroke watcher, so it's explicit. */
    snippetAutoExpandEnabled: boolean;
    /** Lowercased exe names where auto-expand must NOT fire (sensitive apps). */
    snippetAutoExpandExcludedApps: string[];
    /** First-run banner explaining the two global hotkeys (search overlay
     * and clipboard overlay). Once the user dismisses it, this flips to
     * `true` and we never show it again. Default `false` so it surfaces
     * exactly once per install. */
    onboardingHotkeysShown: boolean;
    /** First-run product tour — a 6-step modal that walks the user through
     * the local-first promise, the three global hotkeys (search, clipboard,
     * voice), pinning, categories, and storage location. Flips to `true`
     * once the user finishes (or checks "don't show again"). Default
     * `false` so it surfaces exactly once per install, right after
     * WelcomeSetup completes. */
    tourCompleted: boolean;
    /** Capture images and GIFs alongside text in the clipboard history.
     * Default **on** (changed 2026-05-26 per user verdict — previously
     * default-off "opt-in"; now it ships ready-to-use since a clipboard
     * history that silently drops half the things you copy is the
     * surprising behavior, not the cautious one). The retention default
     * is short (`clipboardImageRetentionDays`) so the disk footprint
     * stays bounded. Existing installs are migrated to `true` exactly
     * once via `_clipboardImagesDefaultV2Applied` below. */
    clipboardImagesEnabled: boolean;
    /** One-time migration sentinel for the 2026-05-26 default flip of
     * `clipboardImagesEnabled` from `false` → `true`. On the first load
     * after upgrading, `coerceSettings` flips any persisted `false` to
     * `true` and sets this flag so the migration runs exactly once.
     * Users who explicitly turn the toggle off *after* the migration
     * have their preference preserved on every subsequent load. */
    _clipboardImagesDefaultV2Applied: boolean;
    /** Days to keep non-pinned image entries before auto-purging. Much
     * shorter than text retention by default since each image is many MB. */
    clipboardImageRetentionDays: number;
    /** Absolute path to the Vosk model directory (e.g.
     * `C:\Users\neo\AppData\...\vosk\model-small-en-us`). Voice
     * recognition is Vosk-only. Empty string until the user
     * downloads or selects a model. */
    voskModelPath: string;
    /** "both" (default) shows the main window + palette. "palette-only" runs
     *  tray-resident (Raycast-style): the main window stays hidden on launch
     *  and only opens on demand from the tray or the palette. Takes effect on
     *  the next launch of KeepItLocal. */
    appMode: 'both' | 'palette-only';
    /** Whether the dedicated voice-dictation overlay's global hotkey
     *  is registered. Default on; users can toggle off if the binding
     *  conflicts with another app. */
    voiceOverlayEnabled: boolean;
    /** The Tauri-style global shortcut to summon the voice overlay.
     *  Default `Ctrl+Alt+V` — V for Voice, Alt to differentiate from
     *  clipboard's Shift+V. Win+V is taken by Windows clipboard. */
    voiceOverlayShortcut: string;
    /** Whether push-to-talk is enabled. PTT is a hold-to-dictate
     *  alternative to the hands-free voice overlay: hold the hotkey,
     *  speak, release — the transcript is pasted into the focused app.
     *  Default OFF — opt-in, since it registers a global hold key. */
    pushToTalkEnabled: boolean;
    /** The Tauri-style global shortcut held to dictate via push-to-talk.
     *  Default `Ctrl+Alt+Space` — Space reads as a natural "talk" key,
     *  Ctrl+Alt keeps it clear of the bare spacebar. */
    pushToTalkShortcut: string;
    /** How dictated text reaches the focused app (voice overlay + PTT):
     *   - `'paste'` — put the transcript on the clipboard and inject
     *     Ctrl+V. Fast and layout-proof, but it overwrites whatever the
     *     user had copied and the dictation lands in clipboard history.
     *   - `'type'`  — synthesize the transcript as keystrokes. Slower for
     *     long text, but the clipboard is never touched (the dictation
     *     stays out of clipboard history) — better for sensitive input.
     *  Default `'paste'` — the long-standing, well-tested path; type-out
     *  is an opt-in. */
    voiceOutputMode: 'paste' | 'type';
    /** #22 — when dictation uses paste mode, restore whatever text the
     *  user had on the clipboard once the target app has consumed the
     *  paste, so dictating does not silently clobber their clipboard.
     *  Only affects paste mode (`type` mode never touches the clipboard).
     *  Default `true` — preserving the clipboard is the polite default. */
    restoreClipboardAfterPaste: boolean;
    /** #23 — opt-in webrtc-vad pre-filter ahead of Vosk on continuous
     *  recognition. A CPU optimization (skips feeding silence) — but a
     *  voice-activity detector can misclassify quiet speech, so it is
     *  OFF by default: continuous dictation then feeds Vosk every chunk,
     *  exactly as before #23. Power users can enable it and tune. */
    voiceVadEnabled: boolean;
    /** Opt-in cadence for the automatic Privacy Audit. The audit is normally
     *  user-initiated only; this is the single owner-approved relaxation,
     *  gated behind explicit opt-in:
     *   - `'off'`    — default. The audit only runs when the user clicks Scan.
     *   - `'launch'` — run once each time the app starts.
     *   - `'daily'`  — run at most once every 24h while the app is open.
     *   - `'weekly'` — run at most once every 7d while the app is open.
     *  Fully local; only raises a notification when it finds un-acknowledged
     *  issues. Scheduling logic lives in `privacyAuditScheduler.ts`. */
    privacyAuditSchedule: 'off' | 'launch' | 'daily' | 'weekly';
}

/** Pick the user's most-likely starting locale before they touch settings.
 * Reads navigator.language (e.g. "ka-GE") and matches the prefix against
 * VALID_LOCALES. Falls back to English when nothing matches — that's safer
 * than guessing wrong and showing a UI the user can't read. */
function detectInitialLocale(): Locale {
    if (typeof navigator === 'undefined') return 'en';
    const raw = (navigator.language || 'en').toLowerCase();
    const prefix = raw.split('-')[0] as Locale;
    return VALID_LOCALES.has(prefix) ? prefix : 'en';
}

const DEFAULTS: AppSettings = {
    theme: 'dark',
    uiFont: 'inter',
    locale: detectInitialLocale(),
    userName: '',
    sidebarCollapsed: false,
    overlayHotkeyMode: 'shortcut',
    overlayHotkeyShortcut: 'CommandOrControl+Alt+S',
    overlayDoubleSpaceIntervalMs: 350,
    commandOverlayEnabled: true,
    commandOverlayShortcut: 'CommandOrControl+Alt+K',
    quickNoteHotkeyEnabled: true,
    quickNoteHotkeyShortcut: 'CommandOrControl+Alt+N',
    appHotkeys: [],
    screenRecordingHotkeyEnabled: true,
    screenRecordingHotkeyShortcut: 'CommandOrControl+Alt+R',
    webSearchEnabled: false,
    browserSearchEnabled: false,
    browserHistoryEnabled: false,
    clipboardOverlayEnabled: true,
    clipboardOverlayShortcut: 'CommandOrControl+Shift+V',
    clipboardAutoPaste: true,
    snippetAutoExpandEnabled: false,
    snippetAutoExpandExcludedApps: [],
    onboardingHotkeysShown: false,
    tourCompleted: false,
    // Default flipped to true on 2026-05-26 — see field comment + the
    // one-time migration in coerceSettings.
    clipboardImagesEnabled: true,
    // Default `false` so the merge-then-coerce migration trips for any
    // install that predates the default flip (their persisted JSON has
    // no entry for this key, so it picks up DEFAULTS' false here).
    _clipboardImagesDefaultV2Applied: false,
    clipboardImageRetentionDays: 2,
    voskModelPath: '',
    appMode: 'both',
    voiceOverlayEnabled: true,
    voiceOverlayShortcut: 'CommandOrControl+Alt+V',
    pushToTalkEnabled: false,
    pushToTalkShortcut: 'CommandOrControl+Alt+Space',
    voiceOutputMode: 'paste',
    restoreClipboardAfterPaste: true,
    voiceVadEnabled: false,
    privacyAuditSchedule: 'off',
};

const STORAGE_KEY = 'keepitlocal_settings_v1';
/** Tauri event used to keep the in-memory settings store in sync across
 * separate webview windows (main window + overlay window each have their own
 * Svelte store instance — without this, toggling a setting in one window
 * never reaches the other until a restart). */
const SETTINGS_UPDATED_EVENT = 'settings-updated';

let backendReady = false;
let backendInitialized = false;
let saveTimeout: ReturnType<typeof setTimeout> | null = null;
let hotkeyTimeout: ReturnType<typeof setTimeout> | null = null;
/** Set while applying a settings payload that came from another window so
 * we don't bounce it back (save → broadcast → receive → save → …). */
let applyingRemoteUpdate = false;

/** Normalize a freshly-merged settings object against the current
 * type contract. Persisted settings.json / localStorage can hold
 * values that were valid in an older build but no longer are — we
 * coerce those back to a safe default here so a stale value never
 * leaks into the now-narrower in-memory type.
 *
 * Handles:
 * - `voiceOutputMode` — anything other than `'paste' | 'type'` falls
 *   back to the `'paste'` default (covers installs predating the
 *   setting, where the merged value is `undefined`).
 * - `clipboardImagesEnabled` v2 default flip (2026-05-26) — exactly
 *   once per install, force the value to `true` and stamp the
 *   `_clipboardImagesDefaultV2Applied` flag. Users who *later* turn
 *   the toggle off keep that choice on every subsequent load (the flag
 *   is true so the migration never runs again).
 *
 * Apply this at every hydration point. */
function coerceSettings(s: AppSettings): AppSettings {
    let next = s;
    // snippetVariables moved to the DPAPI-encrypted backend (see snippets.ts).
    // Strip any legacy copy so it can't be re-persisted to plaintext from here.
    if ('snippetVariables' in (next as unknown as Record<string, unknown>)) {
        next = { ...next };
        delete (next as unknown as Record<string, unknown>).snippetVariables;
    }
    if (next.voiceOutputMode !== 'paste' && next.voiceOutputMode !== 'type') {
        next = { ...next, voiceOutputMode: 'paste' };
    }
    if (!next._clipboardImagesDefaultV2Applied) {
        next = {
            ...next,
            clipboardImagesEnabled: true,
            _clipboardImagesDefaultV2Applied: true,
        };
    }
    return next;
}

function loadLocal(): AppSettings {
    if (typeof localStorage === 'undefined') return DEFAULTS;
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        if (!raw) return DEFAULTS;
        return coerceSettings({ ...DEFAULTS, ...JSON.parse(raw) });
    } catch {
        return DEFAULTS;
    }
}

function saveLocal(s: AppSettings) {
    if (typeof localStorage === 'undefined') return;
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
}

function applyTheme(theme: Theme) {
    if (typeof document !== 'undefined') {
        // Defensive validation — if the persisted settings.json ever holds
        // a theme id that's been removed from THEME_OPTIONS (legacy install,
        // typo, etc.), silently fall back to the default rather than leave
        // the page un-themed with no CSS variables resolved.
        const safe = VALID_THEMES.has(theme) ? theme : 'dark';
        document.documentElement.dataset.theme = safe;
    }
}

/** Apply the chosen UI font by toggling a `data-font` attribute on <html>,
 * mirroring applyTheme. styles.css has a `:root[data-font='<id>']` block per
 * non-default font; the bare `:root` rule is the Inter default, so for
 * `'inter'` we DELETE the attribute rather than set `data-font='inter'` —
 * that keeps the DOM clean and lets the default rule apply. Unknown/legacy
 * values fall back to Inter the same way. */
export function applyFont(font: UiFont) {
    if (typeof document !== 'undefined') {
        const safe = VALID_FONTS.has(font) ? font : 'inter';
        if (safe === 'inter') {
            delete document.documentElement.dataset.font;
        } else {
            document.documentElement.dataset.font = safe;
        }
    }
}

function scheduleBackendSave() {
    if (!backendReady) return;
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(async () => {
        saveTimeout = null;
        const value = get(settings);
        try {
            await invoke('save_app_settings', { settings: value });
        } catch (error) {
            console.warn('Could not persist settings to JSON:', error);
        }
        // Broadcast to all other webviews so their in-memory Svelte store
        // updates immediately. Without this, the overlay window keeps
        // showing stale values until the app restarts.
        try {
            await emit(SETTINGS_UPDATED_EVENT, value);
        } catch (error) {
            console.warn('Could not broadcast settings update:', error);
        }
    }, 150);
}

/** Global shortcuts are app-wide, so exactly ONE window must own their
 *  registration. `+layout.svelte` boots the settings store in EVERY window
 *  (main, command palette, each quick-note, …), so without this gate every
 *  window re-registers the same chords and the 2nd+ registration collides
 *  ("HotKey already registered" / "held by another app") — which gets worse
 *  the more sticky-note windows are open. The main window is the sole owner.
 *  Non-Tauri dev (no window label) defaults to true so local dev still works. */
function ownsGlobalHotkeys(): boolean {
    try {
        return getCurrentWebviewWindow().label === 'main';
    } catch {
        return true;
    }
}

function scheduleHotkeyApply() {
    if (!backendReady) return;
    if (!ownsGlobalHotkeys()) return;
    if (hotkeyTimeout) clearTimeout(hotkeyTimeout);
    hotkeyTimeout = setTimeout(async () => {
        hotkeyTimeout = null;
        const current = get(settings);
        // The standalone search overlay (Ctrl+Alt+S) is retired — the command
        // palette (Ctrl+Alt+K, applied below) is the single search surface, so
        // we no longer register a separate search-overlay hotkey.
        // `apply_overlay_hotkey_config` remains in the backend (unused) in case
        // the overlay is ever re-enabled.
        // The dedicated clipboard-history overlay has its own independent
        // global shortcut. Push it on the same debounce tick so user edits
        // in Settings take effect without a restart.
        try {
            await invoke('apply_clipboard_overlay_hotkey_config', {
                config: {
                    enabled: current.clipboardOverlayEnabled !== false,
                    shortcut: current.clipboardOverlayShortcut || 'CommandOrControl+Shift+V',
                },
            });
        } catch (error) {
            const pretty = prettyShortcut(
                current.clipboardOverlayShortcut || 'CommandOrControl+Shift+V',
            );
            toast(
                `Clipboard overlay hotkey "${pretty}" couldn't be registered — likely held by another app (browsers grab Ctrl+Shift+V for "paste without formatting"). Try a different shortcut in Settings.`,
                'error',
                7000,
            );
            console.warn('Could not apply clipboard overlay hotkey config:', error);
        }
        // Voice overlay — third global hotkey. Follows the same
        // pattern as the other two so a single Settings change
        // reconfigures all three without an app restart.
        try {
            await invoke('apply_voice_overlay_hotkey_config', {
                config: {
                    enabled: current.voiceOverlayEnabled !== false,
                    shortcut: current.voiceOverlayShortcut || 'CommandOrControl+Alt+V',
                },
            });
        } catch (error) {
            const pretty = prettyShortcut(
                current.voiceOverlayShortcut || 'CommandOrControl+Alt+V',
            );
            toast(
                `Voice overlay hotkey "${pretty}" couldn't be registered — likely held by another app. Try a different shortcut in Settings.`,
                'error',
                7000,
            );
            console.warn('Could not apply voice overlay hotkey config:', error);
        }
        // Push-to-talk — the hold-to-dictate hotkey. Off by default, so
        // when disabled this just unregisters whatever was there. Same
        // debounce tick as the overlays so toggling PTT on/off (or
        // remapping it) takes effect without an app restart.
        try {
            await invoke('set_push_to_talk_hotkey', {
                config: {
                    enabled: current.pushToTalkEnabled === true,
                    shortcut: current.pushToTalkShortcut || 'CommandOrControl+Alt+Space',
                },
            });
        } catch (error) {
            const pretty = prettyShortcut(
                current.pushToTalkShortcut || 'CommandOrControl+Alt+Space',
            );
            toast(
                `Push-to-talk hotkey "${pretty}" couldn't be registered — likely held by another app. Try a different shortcut in Settings.`,
                'error',
                7000,
            );
            console.warn('Could not apply push-to-talk hotkey config:', error);
        }
        // Command palette — the unified launcher's global hotkey. Same
        // debounce tick so a Settings change re-registers it without a
        // restart. Backend default + startup registration already exist;
        // this lets the user rebind it in-app.
        try {
            await invoke('apply_command_overlay_hotkey_config', {
                config: {
                    enabled: current.commandOverlayEnabled !== false,
                    shortcut: current.commandOverlayShortcut || 'CommandOrControl+Alt+K',
                },
            });
        } catch (error) {
            const pretty = prettyShortcut(
                current.commandOverlayShortcut || 'CommandOrControl+Alt+K',
            );
            toast(
                `Command palette hotkey "${pretty}" couldn't be registered — likely held by another app. Try a different shortcut in Settings.`,
                'error',
                7000,
            );
            console.warn('Could not apply command overlay hotkey config:', error);
        }
        // Sticky quick-note — the summon-a-note global hotkey. Same debounce
        // tick so rebinding it in Settings takes effect without an app restart.
        try {
            await invoke('apply_quick_note_hotkey_config', {
                config: {
                    enabled: current.quickNoteHotkeyEnabled !== false,
                    shortcut: current.quickNoteHotkeyShortcut || 'CommandOrControl+Alt+N',
                },
            });
        } catch (error) {
            const pretty = prettyShortcut(
                current.quickNoteHotkeyShortcut || 'CommandOrControl+Alt+N',
            );
            toast(
                `Sticky note hotkey "${pretty}" couldn't be registered — likely held by another app. Try a different shortcut in Settings.`,
                'error',
                7000,
            );
            console.warn('Could not apply quick note hotkey config:', error);
        }
        // Screen Recorder is available in the Media category, but it has no supported
        // global hotkey yet. Do not revive its legacy chord during hydration.
        // Per-app hotkeys — a list, not a single chord, so the backend takes
        // the whole set and re-registers it wholesale. It reports partial
        // failures (a chord another app already owns) in the error message
        // while still registering every other row.
        try {
            await invoke('apply_app_hotkeys_config', {
                config: { bindings: current.appHotkeys ?? [] },
            });
        } catch (error) {
            toast(
                `Some per-app hotkeys couldn't be registered — likely held by another app. Try different shortcuts in Settings → Shortcuts.`,
                'error',
                7000,
            );
            console.warn('Could not apply app hotkeys config:', error);
        }
    }, 180);
}

export const settings = writable<AppSettings>(loadLocal());

settings.subscribe((value) => {
    saveLocal(value);
    applyTheme(value.theme);
    applyFont(value.uiFont);
    // When this update arrived from another window, skip the persist+rebroadcast
    // path so we don't ping-pong the change back out.
    if (applyingRemoteUpdate) return;
    scheduleBackendSave();
    scheduleHotkeyApply();
});

export async function initSettingsStore() {
    if (backendInitialized) return;
    backendInitialized = true;

    try {
        const remote = await invoke<Partial<AppSettings>>('load_app_settings');
        applyingRemoteUpdate = true;
        try {
            settings.set(coerceSettings({ ...DEFAULTS, ...remote }));
        } finally {
            applyingRemoteUpdate = false;
        }
    } catch (error) {
        console.warn('Could not load settings JSON, using local fallback:', error);
    } finally {
        backendReady = true;
        scheduleHotkeyApply();
    }

    // Subscribe to cross-window setting broadcasts. Tauri's emit() also
    // delivers to listeners in the originating window, so the applyingRemoteUpdate
    // flag below is what actually breaks the feedback loop — without it, a
    // change here would write to disk → broadcast → re-receive → write → …
    try {
        await listen<AppSettings>(SETTINGS_UPDATED_EVENT, (event) => {
            if (!event.payload) return;
            applyingRemoteUpdate = true;
            try {
                settings.set(coerceSettings({ ...DEFAULTS, ...event.payload }));
            } finally {
                applyingRemoteUpdate = false;
            }
        });
    } catch (error) {
        console.warn('Could not subscribe to settings broadcasts:', error);
    }
}

export function resetSettings() {
    settings.set({ ...DEFAULTS });
}
