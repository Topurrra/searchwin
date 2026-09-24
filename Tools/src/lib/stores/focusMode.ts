/*
  focusMode — a focus session that nudges you off distracting apps AND websites.

  How it works: during a session, a poll (every few seconds) reads the foreground
  window's process name + title (via `get_foreground_window_info`). If the app is
  on your blocklist — or it's a browser whose tab title matches a blocked website
  — KeepItLocal NUDGES (system notification + glow) and, for a blocked *app* in
  "minimize"/"close" mode, acts on the window (`voice_window_action`). A blocked
  *website* is nudge-only: minimizing the whole browser would hide your other
  tabs. Everything is NON-DESTRUCTIVE — close sends a graceful WM_CLOSE, never a
  hard kill. No admin rights required (all user-level Win32 reads).

  The poll also tallies how long the foreground app held focus, so the session
  ends with a private, local summary ("22m VS Code, 2m Chrome") — computed here,
  never sent anywhere. (The always-on Time Tracker captures the same window
  separately for its own dashboard.)

  Session + poll live in this store (not the component) so they persist while you
  navigate. There's always a manual Stop, and the session auto-ends at its
  deadline — never a way to get "stuck" in focus mode.
*/
import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { notifyOS } from './notify';
import { flashGlow } from './glow';

export type FocusAction = 'nudge' | 'minimize' | 'close';

/** How the app lists are interpreted during a session:
 *  - 'blocklist': the listed apps are blocked; everything else stays free.
 *  - 'allowlist': only the listed apps are allowed; everything else is blocked
 *    automatically (deny-by-default — for "let me use ONLY these tools"). */
export type FocusMode = 'blocklist' | 'allowlist';

export interface FocusConfig {
    /** Lowercased process names (no `.exe`), matched against the foreground app. */
    blocklist: string[];
    /** Allow-list counterpart — in allow-list mode, only these are permitted. */
    allowlist: string[];
    /** Lowercased website keywords matched against the foreground browser tab's
     *  title (e.g. "youtube", "reddit", "x.com"). Always enforced on top of the
     *  app list, in either mode. */
    blockedSites: string[];
    /** Which list drives app enforcement this session. */
    mode: FocusMode;
    action: FocusAction;
    durationMin: number;
}

export interface FocusSession {
    active: boolean;
    /** Epoch ms when the session ends. */
    endsAt: number;
}

/** A private, local recap of where focus actually went during a session. */
export interface FocusSummary {
    durationMin: number;
    endedAt: number;
    apps: { app: string; seconds: number }[]; // foreground time per app, desc
    blockedSeconds: number; // time spent on blocked apps/sites during the session
}

const KEY = 'keepitlocal_focus_v1';
const DEFAULTS: FocusConfig = {
    blocklist: [],
    allowlist: [],
    blockedSites: [],
    mode: 'blocklist',
    action: 'minimize',
    durationMin: 25,
};

/** Process names treated as browsers — only these have their tab title checked
 *  against the blocked-sites list. */
const BROWSERS = new Set([
    'chrome',
    'msedge',
    'edge',
    'firefox',
    'brave',
    'opera',
    'opera_gx',
    'vivaldi',
    'arc',
    'chromium',
    'iexplore',
    'librewolf',
    'waterfox',
    'tor',
]);

/** Normalize an app/process name to the form the backend returns: lowercased,
 *  no `.exe`, trimmed. */
export function normalizeApp(name: string): string {
    return name.trim().toLowerCase().replace(/\.exe$/, '');
}

/** Normalize a website entry to a bare keyword: drop scheme, `www.`, and path. */
export function normalizeSite(s: string): string {
    return s
        .trim()
        .toLowerCase()
        .replace(/^https?:\/\//, '')
        .replace(/^www\./, '')
        .replace(/\/.*$/, '');
}

function normalizeList(raw: unknown, fn: (s: string) => string): string[] {
    const arr: unknown[] = Array.isArray(raw) ? raw : [];
    return [
        ...new Set(
            arr.reduce<string[]>((acc, x) => {
                if (typeof x === 'string') {
                    const n = fn(x);
                    if (n) acc.push(n);
                }
                return acc;
            }, []),
        ),
    ];
}

function load(): FocusConfig {
    if (typeof localStorage === 'undefined') return { ...DEFAULTS };
    try {
        const raw = localStorage.getItem(KEY);
        if (!raw) return { ...DEFAULTS };
        const p = JSON.parse(raw);
        return {
            blocklist: normalizeList(p.blocklist, normalizeApp),
            allowlist: normalizeList(p.allowlist, normalizeApp),
            blockedSites: normalizeList(p.blockedSites, normalizeSite),
            mode: p.mode === 'allowlist' ? 'allowlist' : 'blocklist',
            action: p.action === 'nudge' ? 'nudge' : p.action === 'close' ? 'close' : 'minimize',
            durationMin:
                typeof p.durationMin === 'number' && p.durationMin >= 1 && p.durationMin <= 240
                    ? Math.round(p.durationMin)
                    : DEFAULTS.durationMin,
        };
    } catch {
        return { ...DEFAULTS };
    }
}

export const focusConfig = writable<FocusConfig>(load());
focusConfig.subscribe((c) => {
    if (typeof localStorage === 'undefined') return;
    try {
        localStorage.setItem(KEY, JSON.stringify(c));
    } catch {
        // best effort
    }
});

export const focusSession = writable<FocusSession>({ active: false, endsAt: 0 });

/** The most recent finished session's recap (null until the first session ends). */
export const focusLastSummary = writable<FocusSummary | null>(null);

/** Per-key snooze deadlines (epoch ms). App keys are the normalized name; site
 *  keys are `site:<keyword>`. Ephemeral, cleared when a session ends. */
export const focusSnoozes = writable<Record<string, number>>({});

/** Names that are NEVER blocked, even in allow-list mode. Blocking these would
 *  be self-defeating or break the desktop (reach the app to Stop, taskbar,
 *  Start, Alt-Tab, typing). */
const ALWAYS_ALLOWED = new Set([
    'keepitlocal',
    'explorer',
    'searchhost',
    'searchapp',
    'shellexperiencehost',
    'startmenuexperiencehost',
    'textinputhost',
    'applicationframehost',
    'dwm',
    'sihost',
    'ctfmon',
    'lockapp',
]);

/** Decides whether the foreground APP should be enforced against right now.
 *  Block-list: blocked iff on the blocklist. Allow-list: blocked iff NOT on the
 *  allowlist. Safe shell names are always allowed. */
export function isBlockedApp(norm: string, cfg: FocusConfig): boolean {
    if (!norm || ALWAYS_ALLOWED.has(norm)) return false;
    return cfg.mode === 'allowlist' ? !cfg.allowlist.includes(norm) : cfg.blocklist.includes(norm);
}

/** If the foreground window is a browser whose tab title contains a blocked
 *  site keyword, return that keyword; otherwise null. */
export function matchedBlockedSite(norm: string, title: string, cfg: FocusConfig): string | null {
    if (!BROWSERS.has(norm) || !title) return null;
    const t = title.toLowerCase();
    for (const site of cfg.blockedSites) if (site && t.includes(site)) return site;
    return null;
}

let pollId: ReturnType<typeof setInterval> | null = null;
let lastNudge = 0;
let lastGlow = 0;
const NUDGE_THROTTLE_MS = 12_000;
const GLOW_THROTTLE_MS = 2_500;

// Per-session foreground-time tally (for the end-of-session summary).
let tally: Record<string, number> = {};
let blockedTally = 0;
let lastTallyAt = 0;

async function poll() {
    const session = get(focusSession);
    if (!session.active) return;
    if (Date.now() >= session.endsAt) {
        endFocus('Focus session complete — nice work.');
        return;
    }

    let app = '';
    let title = '';
    try {
        const info = await invoke<{ app: string; title: string }>('get_foreground_window_info');
        app = info?.app ?? '';
        title = info?.title ?? '';
    } catch {
        return;
    }

    // Tally foreground time on the current app (capped so a wake-from-sleep gap
    // doesn't inflate the total).
    const now = Date.now();
    const elapsed = lastTallyAt ? Math.min((now - lastTallyAt) / 1000, 10) : 0;
    lastTallyAt = now;
    const norm = normalizeApp(app);
    if (norm && elapsed) tally[norm] = (tally[norm] ?? 0) + elapsed;
    if (!norm) return;

    const cfg = get(focusConfig);
    const appBlocked = isBlockedApp(norm, cfg);
    const site = appBlocked ? null : matchedBlockedSite(norm, title, cfg);
    if (!appBlocked && !site) return;

    // Snooze (escape valve): app snoozes key on the name, site snoozes on `site:`.
    const snoozeKey = site ? `site:${site}` : norm;
    const snoozeUntil = get(focusSnoozes)[snoozeKey];
    if (snoozeUntil && snoozeUntil > Date.now()) return;

    if (elapsed) blockedTally += elapsed;
    const label = site ?? app;

    if (now - lastGlow > GLOW_THROTTLE_MS) {
        lastGlow = now;
        void flashGlow('orange');
    }
    if (now - lastNudge > NUDGE_THROTTLE_MS) {
        lastNudge = now;
        void notifyOS('Focus mode', `${label} is blocked right now — back to it?`, 'focus');
    }

    // A blocked APP can be minimized/closed (non-destructive). A blocked SITE is
    // nudge-only — minimizing the whole browser would hide your other tabs.
    if (appBlocked && (cfg.action === 'minimize' || cfg.action === 'close')) {
        try {
            await invoke('voice_window_action', { action: cfg.action });
        } catch {
            // ignore — the nudge was already delivered
        }
    }
}

function startPoll() {
    stopPoll();
    void poll();
    pollId = setInterval(() => void poll(), 3000);
}
function stopPoll() {
    if (pollId !== null) {
        clearInterval(pollId);
        pollId = null;
    }
}

export function startFocus() {
    const cfg = get(focusConfig);
    const appList = cfg.mode === 'allowlist' ? cfg.allowlist : cfg.blocklist;
    // Need something to enforce. Allow-list mode requires apps (an empty allow
    // list would block everything); block-list mode is fine with apps OR sites.
    const canEnforce =
        cfg.mode === 'allowlist' ? appList.length > 0 : appList.length > 0 || cfg.blockedSites.length > 0;
    if (!canEnforce) return;

    lastNudge = 0;
    lastGlow = 0;
    tally = {};
    blockedTally = 0;
    lastTallyAt = 0;
    focusSession.set({ active: true, endsAt: Date.now() + cfg.durationMin * 60_000 });

    const parts: string[] = [];
    if (cfg.mode === 'allowlist') parts.push(`only ${appList.length} app(s) allowed`);
    else if (appList.length) parts.push(`${appList.length} app(s)`);
    if (cfg.blockedSites.length) parts.push(`${cfg.blockedSites.length} site(s)`);
    void notifyOS('Focus mode on', `Blocking ${parts.join(' + ')} for ${cfg.durationMin} min.`, 'focus');
    startPoll();
}

export function endFocus(reason?: string) {
    const session = get(focusSession);
    const cfg = get(focusConfig);
    stopPoll();

    // Capture the recap before clearing the tallies.
    if (session.active) {
        const apps = Object.entries(tally)
            .map(([app, seconds]) => ({ app, seconds: Math.round(seconds) }))
            .filter((a) => a.seconds >= 1)
            .sort((a, b) => b.seconds - a.seconds);
        focusLastSummary.set({
            durationMin: cfg.durationMin,
            endedAt: Date.now(),
            apps,
            blockedSeconds: Math.round(blockedTally),
        });
    }

    focusSession.set({ active: false, endsAt: 0 });
    focusSnoozes.set({}); // snoozes are session-scoped
    tally = {};
    blockedTally = 0;
    lastTallyAt = 0;
    if (reason) void notifyOS('Focus mode off', reason, 'focus');
}

/** Temporarily allow a blocked app/site for `minutes` (default 3). */
export function snoozeApp(name: string, minutes = 3) {
    const n = normalizeApp(name);
    if (!n) return;
    focusSnoozes.update((s) => ({ ...s, [n]: Date.now() + minutes * 60_000 }));
}
export function snoozeSite(site: string, minutes = 3) {
    const n = normalizeSite(site);
    if (!n) return;
    focusSnoozes.update((s) => ({ ...s, [`site:${n}`]: Date.now() + minutes * 60_000 }));
}

/** Cancel a snooze (re-block immediately). */
export function unsnoozeApp(name: string) {
    const n = normalizeApp(name);
    focusSnoozes.update((s) => {
        const next = { ...s };
        delete next[n];
        return next;
    });
}

// ─── List management ───────────────────────────────────────────────────────
export function setFocusMode(mode: FocusMode) {
    focusConfig.update((c) => ({ ...c, mode }));
}
export function addToBlocklist(name: string) {
    const n = normalizeApp(name);
    if (!n) return;
    focusConfig.update((c) => (c.blocklist.includes(n) ? c : { ...c, blocklist: [...c.blocklist, n] }));
}
export function removeFromBlocklist(name: string) {
    const n = normalizeApp(name);
    focusConfig.update((c) => ({ ...c, blocklist: c.blocklist.filter((x) => x !== n) }));
}
export function addToAllowlist(name: string) {
    const n = normalizeApp(name);
    if (!n) return;
    focusConfig.update((c) => (c.allowlist.includes(n) ? c : { ...c, allowlist: [...c.allowlist, n] }));
}
export function removeFromAllowlist(name: string) {
    const n = normalizeApp(name);
    focusConfig.update((c) => ({ ...c, allowlist: c.allowlist.filter((x) => x !== n) }));
}
export function addBlockedSite(site: string) {
    const n = normalizeSite(site);
    if (!n) return;
    focusConfig.update((c) => (c.blockedSites.includes(n) ? c : { ...c, blockedSites: [...c.blockedSites, n] }));
}
export function removeBlockedSite(site: string) {
    const n = normalizeSite(site);
    focusConfig.update((c) => ({ ...c, blockedSites: c.blockedSites.filter((x) => x !== n) }));
}
export function setFocusAction(action: FocusAction) {
    focusConfig.update((c) => ({ ...c, action }));
}
export function setFocusDuration(durationMin: number) {
    const d = Math.max(1, Math.min(240, Math.round(durationMin)));
    focusConfig.update((c) => ({ ...c, durationMin: d }));
}
