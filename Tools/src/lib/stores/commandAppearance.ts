/**
 * Command-palette appearance preferences — user-tunable look of the /command
 * surface ONLY (not the rest of the app). Kept deliberately OUT of the main
 * `AppSettings` store: the Rust `AppSettingsPayload` is a strict typed struct
 * that drops unknown fields on its save round-trip, so a dedicated
 * localStorage-backed store is the clean, backend-free home for these prefs.
 *
 * Cross-window sync: localStorage is shared across all webview windows of the
 * app, and the `storage` event fires in OTHER windows when it changes — so
 * moving the slider in the main window's Settings live-updates the separate
 * command-palette window without any Tauri event plumbing.
 *
 * Expanded for the Palette Appearance phase (2026-05-27) — adds theme,
 * density, animation level, accent-glow, window width/position, hidden
 * sections. Loader is migration-aware: legacy v1 blobs with only the old
 * three fields hydrate cleanly using the new defaults.
 */
import { writable } from 'svelte/store';

// Theme is owned by the app-wide settings store (`settings.ts` — `Theme`
// type + THEME_OPTIONS). The palette's Appearance editor exposes the
// app-wide theme picker rather than holding a separate copy: picking
// "Midnight" should change BOTH the palette and the rest of the app
// to keep them visually coherent (no Sepia-palette-over-Dark-app
// weirdness). See settings.ts for the canonical theme list.

// ─── Enumerations (kept narrow so the loader can validate at runtime) ───

/** Row density. Adjusts row height + inter-row gap via CSS variables. */
export type PaletteDensity = 'compact' | 'cozy' | 'comfortable';

export const DENSITY_OPTIONS: { id: PaletteDensity; label: string; rowPx: number }[] = [
    // 2026-07-16: rescaled +10px for the V2 row proportions. V2's polish pass
    // hardcoded 42px rows, which silently dead-ended this control; wiring it
    // back up on the old 28/32/36 scale would have shrunk every row. Cozy (the
    // default) is 42 so V2 looks exactly as it does today.
    { id: 'compact', label: 'Compact', rowPx: 36 },
    { id: 'cozy', label: 'Cozy', rowPx: 42 },
    { id: 'comfortable', label: 'Comfortable', rowPx: 48 },
];

/** Window width preset. Mapped to the Tauri palette window size on apply.
 *  Wave I (2026-05-27): dropped Compact (460 px) — the search input got
 *  cramped at that width with the chip row + accent icons. Three sizes
 *  with clear separation cover every reasonable monitor.
 *  2026-07-30: Standard bumped 640 → 700 — the chip row grew to ten chips
 *  (Windows + Emoji) and "Commands" fell off the right edge at 640. */
export type PaletteWidth = 'standard' | 'wide' | 'extra-wide';

export const WIDTH_OPTIONS: { id: PaletteWidth; label: string; widthPx: number }[] = [
    { id: 'standard', label: 'Standard', widthPx: 700 },
    { id: 'wide', label: 'Wide', widthPx: 820 },
    { id: 'extra-wide', label: 'Extra-wide', widthPx: 1000 },
];

/** Window position. Wave I (2026-05-27): pruned to the three picks that
 *  matter — top / center / bottom. The "top-third" and "bottom-third"
 *  middle options felt indistinguishable from the extreme picks on a
 *  1080p screen, and "remember last" added a config knob most users
 *  don't reach for. Keep it sharp. */
export type PalettePosition =
    | 'top'    // 40 px from the top edge — Spotlight feel
    | 'center' // dead-center of the focused monitor
    | 'bottom'; // 40 px from the bottom edge

/** Animation intensity. CSS `--motion-multiplier` scales every transition
 *  duration so the same easing curve feels snappier or springier.
 *  Wave H: lifted the dynamic range so the difference between options
 *  is genuinely visible (was: 0 / 1 / 1.6 — too subtle; now: 0 / 1 / 2.4). */
export type PaletteAnimationLevel = 'reduced' | 'default' | 'lively';

export const ANIMATION_OPTIONS: { id: PaletteAnimationLevel; label: string; mult: number }[] = [
    { id: 'reduced', label: 'Reduced', mult: 0 }, // instant — honors prefers-reduced-motion in spirit
    { id: 'default', label: 'Default', mult: 1 },
    { id: 'lively', label: 'Lively', mult: 2.4 }, // 2.4× — feels distinctly leisurely + spring-like
];

/** Sections in the palette body the user may hide. Each id corresponds to
 *  a section rendered in `routes/command/PaletteV2.svelte`. */
export type PaletteSectionId =
    | 'reminders'
    | 'quick-notes'
    | 'time-focus'
    | 'recent-items'
    | 'recent-folders'
    | 'settings'
    | 'suggested-tools';

export const HIDABLE_SECTIONS: { id: PaletteSectionId; label: string; hint: string }[] = [
    { id: 'reminders', label: 'Reminders', hint: 'Inline "remind me" parser results' },
    { id: 'quick-notes', label: 'Quick Notes', hint: 'Type-as-note shortcut' },
    { id: 'time-focus', label: 'Time & Focus', hint: 'Start/stop tracking from the palette' },
    { id: 'recent-items', label: 'Recent items', hint: 'Last opened files / tools' },
    { id: 'recent-folders', label: 'Recent folders', hint: 'Last visited folders' },
    { id: 'settings', label: 'Settings shortcuts', hint: 'Jump-to-settings rows' },
    { id: 'suggested-tools', label: 'Suggested apps', hint: 'Empty-state app shortcuts' },
];

/** Accent color presets. The custom color picker stays available alongside
 *  these for power users who want an exact hex. */
export interface AccentPreset {
    id: string;
    label: string;
    hex: string;
}

export const ACCENT_PRESETS: AccentPreset[] = [
    { id: 'emerald', label: 'Emerald', hex: '#10b981' },
    { id: 'blue', label: 'Blue', hex: '#3b82f6' },
    { id: 'purple', label: 'Purple', hex: '#8b5cf6' },
    { id: 'pink', label: 'Pink', hex: '#ec4899' },
    { id: 'amber', label: 'Amber', hex: '#f59e0b' },
    { id: 'red', label: 'Red', hex: '#ef4444' },
    { id: 'cyan', label: 'Cyan', hex: '#06b6d4' },
    { id: 'slate', label: 'Slate', hex: '#94a3b8' },
];

// ─── The store shape ────────────────────────────────────────────────────

export interface CommandAppearance {
    /** Panel surface opacity, 0..1. 1 = fully solid; lower lets more of
     *  whatever is behind the palette tint through (glassier). */
    opacity: number;
    /** Accent color override (any CSS color string) scoped to the palette,
     *  or null to inherit the active theme's accent. */
    accent: string | null;
    /** Ask the backend to apply a real Windows DWM acrylic blur to the
     *  command window so the desktop behind it is blurred. When ON, the
     *  palette automatically switches to SQUARE corners (CSS + DWM) to
     *  avoid the dark-wedge leak around rounded corners. */
    desktopBlur: boolean;
    /** Row density (compact/cozy/comfortable). */
    density: PaletteDensity;
    /** Animation intensity — scales every transition duration via a CSS
     *  multiplier. */
    animationLevel: PaletteAnimationLevel;
    /** Subtle outer-glow around selected rows + focus rings. */
    accentGlow: boolean;
    /** Window width preset. The frontend applies it via Tauri's window
     *  setSize on open + when changed live. */
    width: PaletteWidth;
    /** Window position preset. */
    position: PalettePosition;
    /** Last user-positioned XY (set when position === 'remember-last'). */
    rememberedX: number | null;
    rememberedY: number | null;
    /** Section ids the user has chosen to hide. */
    hiddenSections: PaletteSectionId[];
    /** Wave H (2026-05-27): show or hide the category chip row below
     *  the search input. Users who never use chips can hide them for a
     *  cleaner palette; defaults to ON so the feature is discoverable. */
    showChips: boolean;
}

const KEY = 'keepitlocal_command_appearance_v1';

export const APPEARANCE_DEFAULTS: CommandAppearance = {
    opacity: 0.93,
    accent: null,
    desktopBlur: false,
    density: 'cozy',
    animationLevel: 'default',
    accentGlow: true,
    width: 'standard',
    position: 'center',
    rememberedX: null,
    rememberedY: null,
    hiddenSections: [],
    showChips: true,
};

/** Opacity is clamped to a sane range: below ~0.6 the rows get hard to
 *  read over a busy desktop; 1.0 is fully solid. */
export const OPACITY_MIN = 0.6;
export const OPACITY_MAX = 1;

// ─── Sanitizers (defense against junk in localStorage) ──────────────────

function clampOpacity(value: unknown): number {
    if (typeof value !== 'number' || Number.isNaN(value)) {
        return APPEARANCE_DEFAULTS.opacity;
    }
    return Math.min(OPACITY_MAX, Math.max(OPACITY_MIN, value));
}

function sanitizeAccent(value: unknown): string | null {
    if (typeof value !== 'string') return null;
    const trimmed = value.trim();
    if (!trimmed) return null;
    // Defensive cap — a color string is short; anything long is junk and we
    // never want to inject an arbitrary blob into an inline style.
    if (trimmed.length > 32) return null;
    return trimmed;
}

function isDensity(value: unknown): value is PaletteDensity {
    return DENSITY_OPTIONS.some((d) => d.id === value);
}
function isWidth(value: unknown): value is PaletteWidth {
    return WIDTH_OPTIONS.some((w) => w.id === value);
}
function isPosition(value: unknown): value is PalettePosition {
    return value === 'top' || value === 'center' || value === 'bottom';
}
function isAnimationLevel(value: unknown): value is PaletteAnimationLevel {
    return ANIMATION_OPTIONS.some((a) => a.id === value);
}
function isSectionId(value: unknown): value is PaletteSectionId {
    return HIDABLE_SECTIONS.some((s) => s.id === value);
}
function sanitizeHiddenSections(value: unknown): PaletteSectionId[] {
    if (!Array.isArray(value)) return [];
    const seen = new Set<PaletteSectionId>();
    for (const v of value) if (isSectionId(v)) seen.add(v);
    return [...seen];
}
function sanitizeRememberedCoord(value: unknown): number | null {
    if (typeof value !== 'number' || !Number.isFinite(value)) return null;
    // Defensive bounds — anything off-screen by miles is probably stale.
    if (value < -10_000 || value > 30_000) return null;
    return Math.round(value);
}

function load(): CommandAppearance {
    if (typeof localStorage === 'undefined') return { ...APPEARANCE_DEFAULTS };
    try {
        const raw = localStorage.getItem(KEY);
        if (!raw) return { ...APPEARANCE_DEFAULTS };
        const parsed = JSON.parse(raw) as Partial<CommandAppearance>;
        return {
            opacity: clampOpacity(parsed?.opacity),
            accent: sanitizeAccent(parsed?.accent),
            desktopBlur: parsed?.desktopBlur === true,
            density: isDensity(parsed?.density) ? parsed.density : APPEARANCE_DEFAULTS.density,
            animationLevel: isAnimationLevel(parsed?.animationLevel)
                ? parsed.animationLevel
                : APPEARANCE_DEFAULTS.animationLevel,
            accentGlow: parsed?.accentGlow !== false, // default true; only false suppresses
            width: isWidth(parsed?.width) ? parsed.width : APPEARANCE_DEFAULTS.width,
            position: isPosition(parsed?.position) ? parsed.position : APPEARANCE_DEFAULTS.position,
            rememberedX: sanitizeRememberedCoord(parsed?.rememberedX),
            rememberedY: sanitizeRememberedCoord(parsed?.rememberedY),
            hiddenSections: sanitizeHiddenSections(parsed?.hiddenSections),
            showChips: parsed?.showChips !== false, // default true
        };
    } catch {
        return { ...APPEARANCE_DEFAULTS };
    }
}

export const commandAppearance = writable<CommandAppearance>(load());

function persist(value: CommandAppearance) {
    if (typeof localStorage === 'undefined') return;
    try {
        localStorage.setItem(KEY, JSON.stringify(value));
    } catch {
        // localStorage unavailable — keep the in-memory value only.
    }
}

// While applying a value that arrived from another window we must NOT persist
// it back (write → storage event → set → write → …).
let applyingExternal = false;

commandAppearance.subscribe((value) => {
    if (applyingExternal) return;
    persist(value);
});

if (typeof window !== 'undefined') {
    window.addEventListener('storage', (event) => {
        if (event.key !== KEY) return;
        applyingExternal = true;
        commandAppearance.set(load());
        applyingExternal = false;
    });
}

// ─── Setters ────────────────────────────────────────────────────────────

export function setCommandOpacity(opacity: number) {
    commandAppearance.update((s) => ({ ...s, opacity: clampOpacity(opacity) }));
}

export function setCommandAccent(accent: string | null) {
    commandAppearance.update((s) => ({ ...s, accent: sanitizeAccent(accent) }));
}

export function setCommandDesktopBlur(enabled: boolean) {
    commandAppearance.update((s) => ({ ...s, desktopBlur: enabled === true }));
}

export function setCommandDensity(density: PaletteDensity) {
    if (!isDensity(density)) return;
    commandAppearance.update((s) => ({ ...s, density }));
}

export function setCommandAnimationLevel(level: PaletteAnimationLevel) {
    if (!isAnimationLevel(level)) return;
    commandAppearance.update((s) => ({ ...s, animationLevel: level }));
}

export function setCommandAccentGlow(enabled: boolean) {
    commandAppearance.update((s) => ({ ...s, accentGlow: enabled === true }));
}

export function setCommandWidth(width: PaletteWidth) {
    if (!isWidth(width)) return;
    commandAppearance.update((s) => ({ ...s, width }));
}

export function setCommandPosition(position: PalettePosition) {
    if (!isPosition(position)) return;
    commandAppearance.update((s) => ({ ...s, position }));
}

/** Stash the user-dragged position; only meaningful while
 *  `position === 'remember-last'`. Coords are clamped to a sane range. */
export function setCommandRememberedPosition(x: number, y: number) {
    commandAppearance.update((s) => ({
        ...s,
        rememberedX: sanitizeRememberedCoord(x),
        rememberedY: sanitizeRememberedCoord(y),
    }));
}

export function setCommandShowChips(show: boolean) {
    commandAppearance.update((s) => ({ ...s, showChips: show === true }));
}

export function toggleHiddenSection(id: PaletteSectionId) {
    if (!isSectionId(id)) return;
    commandAppearance.update((s) => {
        const has = s.hiddenSections.includes(id);
        return {
            ...s,
            hiddenSections: has
                ? s.hiddenSections.filter((x) => x !== id)
                : [...s.hiddenSections, id],
        };
    });
}

export function resetCommandAppearance() {
    commandAppearance.set({ ...APPEARANCE_DEFAULTS });
}

// ─── Bulk apply (used by Style Presets in Wave E) ───────────────────────

/** Apply a partial appearance patch — used by the named-preset row in the
 *  Appearance editor. Validation runs per-field so an invalid value in the
 *  patch just gets skipped, not crashes the whole apply. */
export function applyAppearancePatch(patch: Partial<CommandAppearance>) {
    commandAppearance.update((s) => ({
        ...s,
        ...(patch.opacity !== undefined ? { opacity: clampOpacity(patch.opacity) } : {}),
        ...(patch.accent !== undefined ? { accent: sanitizeAccent(patch.accent) } : {}),
        ...(patch.desktopBlur !== undefined ? { desktopBlur: patch.desktopBlur === true } : {}),
        ...(isDensity(patch.density) ? { density: patch.density } : {}),
        ...(isAnimationLevel(patch.animationLevel)
            ? { animationLevel: patch.animationLevel }
            : {}),
        ...(patch.accentGlow !== undefined ? { accentGlow: patch.accentGlow === true } : {}),
        ...(isWidth(patch.width) ? { width: patch.width } : {}),
        ...(isPosition(patch.position) ? { position: patch.position } : {}),
        ...(Array.isArray(patch.hiddenSections)
            ? { hiddenSections: sanitizeHiddenSections(patch.hiddenSections) }
            : {}),
        ...(patch.showChips !== undefined ? { showChips: patch.showChips === true } : {}),
    }));
}
