/*
  privacyAuditScheduler — the OPT-IN automatic Privacy Audit.

  The Privacy Audit is otherwise user-initiated only (you click "Scan"). This
  module is the single, owner-approved relaxation of that rule, gated behind an
  explicit opt-in: when the user picks a cadence in Settings → System →
  "Automatic privacy audit" (off by default), we run the SAME local scan on that
  schedule and raise a local notification ONLY when un-acknowledged issues are
  found. Everything is local — the scan makes zero network I/O, and we never
  upload anything.

  It runs the SAME shared scan as the page ("Scan" button), `runPrivacyAuditScan`
  in stores/privacyAudit.ts, so a scheduled run populates the exact stores the
  Privacy Audit page renders — open the page after a scheduled run and the
  findings are already there (no empty first-run hero).

  Cadence semantics:
    • 'launch'  — once per app session, shortly after launch.
    • 'daily'   — at most once per 24h.
    • 'weekly'  — at most once per 7d.

  Two rules keep a scheduled run from disrupting the user:
    1. NEVER fires at the instant you enable it. The first time we see a daily/
       weekly cadence with no recorded run, we START THE CLOCK (seed the
       timestamp) instead of running — so enabling "daily" today does NOT scan
       today, and definitely not on the launch you enabled it on.
    2. A launch-time due run is DEFERRED past the startup window (the scans are
       already off the main thread, but app launch is I/O-heavy, so a burst of
       disk/registry/process work then would read as a stutter). The periodic
       in-session recheck runs immediately — the app is settled by then.

  The app is tray-resident, so a frontend-driven scheduler in the long-lived
  main window is sufficient; there is no OS-level scheduled task. Due-ness is
  re-checked on init, on a 60-minute interval, and whenever the user changes the
  schedule, using a single localStorage "last run" timestamp.

  Guarded to the MAIN window only — mirrors the global-hotkey ownership gate so
  overlay / command-palette / quick-note webviews never run the audit.

  Best-effort + no-throw: a failed scheduled run must never crash or block the
  UI, so the run is wrapped in try/catch.
*/
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { get } from 'svelte/store';
import { settings } from '$lib/stores/settings';
import { notifyOS } from '$lib/stores/notify';
import { runPrivacyAuditScan } from '$lib/stores/privacyAudit';

/** localStorage key holding the last scheduled-run timestamp (ms since epoch).
 *  Persisted (not session) so daily/weekly cadences survive app restarts. */
const LAST_RUN_KEY = 'keepitlocal_privacy_audit_last_run';

const ONE_HOUR_MS = 60 * 60 * 1000;
const ONE_DAY_MS = 24 * ONE_HOUR_MS;
const ONE_WEEK_MS = 7 * ONE_DAY_MS;

/** How often we re-check due-ness while the app stays open (covers daily /
 *  weekly cadences crossing their boundary mid-session). */
const RECHECK_INTERVAL_MS = ONE_HOUR_MS;

/** Delay before a launch-time (or settings-change) DUE run actually fires, so
 *  it never competes with the I/O-heavy app-launch window. The in-session
 *  recheck does NOT defer — the app is already settled by then. */
const LAUNCH_DEFER_MS = 90 * 1000;

let recheckTimer: ReturnType<typeof setInterval> | null = null;
let deferTimer: ReturnType<typeof setTimeout> | null = null;
let unsubscribeSettings: (() => void) | null = null;
/** Set once we've kicked off a `'launch'` run this app session, so we don't
 *  re-run on launch every time the settings subscriber fires. */
let launchRanThisSession = false;
/** Re-entrancy guard so an in-flight run can't be started twice (e.g. the
 *  interval tick racing a settings change). */
let runInFlight = false;

/** Only the main window owns the scheduler — overlays / palette / quick-notes
 *  are separate webviews and must never run the audit. Mirrors
 *  `ownsGlobalHotkeys()` in settings.ts. Non-Tauri dev (no window label)
 *  defaults to false so a plain browser never tries to invoke the command. */
function isMainWindow(): boolean {
    try {
        return getCurrentWebviewWindow().label === 'main';
    } catch {
        return false;
    }
}

function readLastRun(): number {
    if (typeof localStorage === 'undefined') return 0;
    try {
        const raw = localStorage.getItem(LAST_RUN_KEY);
        const n = raw ? Number(raw) : 0;
        return Number.isFinite(n) && n > 0 ? n : 0;
    } catch {
        return 0;
    }
}

function writeLastRun(ms: number): void {
    if (typeof localStorage === 'undefined') return;
    try {
        localStorage.setItem(LAST_RUN_KEY, String(ms));
    } catch {
        // Storage unavailable / quota — non-fatal; worst case we re-run sooner.
    }
}

/** Is a scheduled run due right now for the given cadence? */
function isDue(schedule: string, now: number): boolean {
    const last = readLastRun();
    switch (schedule) {
        case 'launch':
            // Run once per app session. The persisted timestamp isn't enough
            // (a daily run yesterday shouldn't suppress a launch run today),
            // so we track session state in-memory.
            return !launchRanThisSession;
        case 'daily':
            return now - last >= ONE_DAY_MS;
        case 'weekly':
            return now - last >= ONE_WEEK_MS;
        default:
            return false;
    }
}

/** Actually run the audit (guarded), populate the shared store, and notify on
 *  un-acknowledged issues. Re-validates due-ness so a deferred timer that fires
 *  after the schedule changed (or after another run already happened) no-ops.
 *  No-throw. */
async function runNow(): Promise<void> {
    if (runInFlight) return;
    const schedule = get(settings).privacyAuditSchedule;
    if (schedule === 'off' || !isMainWindow()) return;

    const now = Date.now();
    if (!isDue(schedule, now)) return;

    runInFlight = true;
    // Stamp BEFORE awaiting so a slow run can't be double-triggered by the next
    // interval tick, and a failed run doesn't hammer the scan every minute.
    if (schedule === 'launch') launchRanThisSession = true;
    writeLastRun(now);

    try {
        // SAME scan the page's "Scan" button runs — this populates the Privacy
        // Audit stores, so opening the page shows the scheduled results.
        const summary = await runPrivacyAuditScan();
        if (summary && summary.totalUnacknowledged > 0) {
            const n = summary.totalUnacknowledged;
            const noun = n === 1 ? 'issue' : 'issues';
            let body = `${n} ${noun} found — open KeepItLocal to review`;
            if (summary.high > 0) {
                const h = summary.high === 1 ? '1 high-severity' : `${summary.high} high-severity`;
                body = `${n} ${noun} found (${h}) — open KeepItLocal to review`;
            }
            // Reuse the app's notification channel (overlay toast + a real,
            // permission-checked OS notification when the window isn't
            // foreground). 'warn' tints the toast amber.
            await notifyOS('Privacy audit', body, 'warn');
        }
        // 0 issues → silent all-clear; we never notify on a clean run.
    } catch (error) {
        // A failed scheduled run must never disrupt the app. Roll back the
        // launch flag so the next due-check can retry this session.
        if (schedule === 'launch') launchRanThisSession = false;
        console.warn('Scheduled privacy audit failed:', error);
    } finally {
        runInFlight = false;
    }
}

/** Decide whether a run is due and either run it now or defer it past launch.
 *  `defer` = true for launch / settings-change checks (push the run past the
 *  startup window); false for the periodic in-session recheck. No-throw. */
function maybeRun(defer: boolean): void {
    const schedule = get(settings).privacyAuditSchedule;
    if (schedule === 'off') return;
    if (!isMainWindow()) return;

    // Rule 1 — seed, don't run: the first time we ever see a daily/weekly
    // cadence with no recorded run, START THE CLOCK instead of scanning, so
    // enabling "daily" today doesn't fire today (or at this launch). The first
    // real run then lands one full period later.
    if ((schedule === 'daily' || schedule === 'weekly') && readLastRun() === 0) {
        writeLastRun(Date.now());
        return;
    }

    if (!isDue(schedule, Date.now())) return;

    if (defer) {
        // Rule 2 — push a launch-time due run past the startup window. Only one
        // deferred run is ever queued; runNow re-validates due-ness when it fires.
        if (deferTimer === null) {
            deferTimer = setTimeout(() => {
                deferTimer = null;
                void runNow();
            }, LAUNCH_DEFER_MS);
        }
    } else {
        void runNow();
    }
}

/** Tear down the recheck interval (without dropping the settings subscription).
 *  Used when the schedule is toggled to 'off'. */
function clearRecheckTimer(): void {
    if (recheckTimer !== null) {
        clearInterval(recheckTimer);
        recheckTimer = null;
    }
}

/** Cancel a pending deferred launch run. */
function clearDeferTimer(): void {
    if (deferTimer !== null) {
        clearTimeout(deferTimer);
        deferTimer = null;
    }
}

/** Re-evaluate after a settings change (or on init): start/stop the timers to
 *  match the current cadence, then check if anything is due. Launch / settings-
 *  change checks DEFER a due run past the startup window. No-throw. */
function reevaluate(): void {
    const schedule = get(settings).privacyAuditSchedule;
    if (schedule === 'off') {
        clearRecheckTimer();
        clearDeferTimer();
        return;
    }
    // Keep the recheck interval alive for daily/weekly cadences crossing their
    // boundary mid-session. ('launch' never needs it, but an idle 60-min timer
    // is harmless and keeps the logic uniform.) The interval runs WITHOUT defer.
    if (recheckTimer === null) {
        recheckTimer = setInterval(() => {
            maybeRun(false);
        }, RECHECK_INTERVAL_MS);
    }
    maybeRun(true);
}

/** Start the opt-in Privacy Audit scheduler. Safe to call once from the main
 *  window's onMount — no-ops in any other window. Idempotent. No-throw. */
export async function initPrivacyAuditScheduler(): Promise<void> {
    if (!isMainWindow()) return;
    if (unsubscribeSettings !== null) return; // already initialized

    try {
        // Re-evaluate whenever the schedule changes in Settings (or on any
        // settings update — `reevaluate` is cheap and idempotent). Svelte's
        // subscribe fires immediately with the current value, which also
        // performs the initial (launch) due-check on init.
        unsubscribeSettings = settings.subscribe(() => {
            reevaluate();
        });
    } catch (error) {
        console.warn('Could not start privacy audit scheduler:', error);
    }
}

/** Stop the scheduler and release its timers + subscription. Called from the
 *  main window's onDestroy. No-throw / idempotent. */
export function stopPrivacyAuditScheduler(): void {
    clearRecheckTimer();
    clearDeferTimer();
    if (unsubscribeSettings !== null) {
        try {
            unsubscribeSettings();
        } catch {
            // ignore
        }
        unsubscribeSettings = null;
    }
}
