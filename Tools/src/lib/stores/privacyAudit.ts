/*
  privacyAudit — scan state for the Privacy Audit tool.

  The Privacy Audit page is mounted inside the workspace's `{#key}` block, so
  navigating away and back UNMOUNTS the component and wipes any component-local
  `$state`. A finished scan would therefore vanish the moment the user clicked
  another tool. These module-level stores live for the app session, so the
  results, the open/closed accordion state, the file-preview cache, and the
  first-run flag all survive navigation. (Acknowledgements are a separate,
  backend-persisted concern — see privacyAck.ts.)

  This store also drives per-scan streaming: `results` is seeded with one
  placeholder entry per scan marked `loading`, and each backend command updates
  ONLY its own entry as it resolves, so fast scans render immediately instead of
  waiting on the slowest one.
*/

import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import type { SensitiveFinding } from '$lib/types/sensitive';
import { acknowledged } from './privacyAck';

export type Severity = 'ok' | 'info' | 'low' | 'medium' | 'high';

export interface FindingFix {
    label: string;
    action: 'open-setting' | 'reveal-file' | 'reveal-path';
    target: string;
}

export interface Finding {
    id: string;
    category: string;
    severity: Severity;
    title: string;
    detail: string;
    recommendation: string;
    fix?: FindingFix;
    /** Phase 6.5-5: kind labels (e.g. ['aws_access_key', 'openai_api_key'])
     *  for secrets findings. Drives the per-finding chips so the user
     *  sees WHAT triggered the flag at a glance. */
    kinds?: string[];
    /** Phase 6.5-5: file path for inline Preview. When present, a
     *  Preview button on the finding card opens an expandable panel
     *  with the file's content rendered through PrivacyBlur. */
    previewPath?: string;
}

/** One excerpt of file content around a hit — Phase 6.5-5 revision.
 *  The backend trims the file to just the matched lines (±2 context
 *  lines, overlapping windows merged) so the preview shows only
 *  what triggered the scanner, not a whole 5000-line config. */
export interface PreviewExcerpt {
    lineStart: number;
    content: string;
    findings: SensitiveFinding[];
}

/** Per-finding preview state — keyed by finding id. */
export interface FilePreview {
    excerpts: PreviewExcerpt[];
    totalFindings: number;
    truncated: boolean;
    error: string | null;
}

export interface ScanResult {
    scan: string;
    findings: Finding[];
    summary: string;
    unavailable: boolean;
    /** True while this scan's backend command is still in flight. Each scan
     *  clears its own flag as it resolves so its card stops spinning the moment
     *  its result is in — independent of the other (possibly slower) scans. */
    loading: boolean;
}

/** All scan cards in display order. Seeded with placeholder `loading` entries
 *  at scan start, then each entry is replaced in place as its command resolves. */
export const results = writable<ScanResult[]>([]);

/** True once a scan has been STARTED this session — drives the first-run hero
 *  (shown only while false). Set the instant a scan kicks off so the hero is
 *  replaced by streaming cards immediately, not after all scans finish. */
export const ranOnce = writable(false);

/** True while at least one scan command is still in flight — drives the
 *  Re-scan / hero button's loading state and guards against concurrent runs. */
export const scanning = writable(false);

/** Top-level error (only set if the whole dispatch throws). Individual scan
 *  failures are recorded on their own ScanResult entry, not here. */
export const errorMsg = writable<string | null>(null);

/** Which scan sections are expanded, keyed by scan id. */
export const openSections = writable<Record<string, boolean>>({});

/** Which finding preview panels are open, keyed by finding id. */
export const previewOpen = writable<Record<string, boolean>>({});

/** Memoized file-preview fetches, keyed by finding id. `'loading'` marks an
 *  in-flight request so the UI can show a spinner. */
export const previewCache = writable<Record<string, FilePreview | 'loading'>>({});

// ─── Shared scan runner ─────────────────────────────────────────────────────
// The dispatch list (scan id → backend command). The Privacy Audit PAGE keeps
// its own display list (icon + title per id) in PrivacyAudit.svelte; THIS list
// is the single source of truth for WHICH scans run and HOW they're invoked.
// Keep the two id sets in sync.
export const PRIVACY_SCANS: { id: string; command: string }[] = [
    { id: 'mic-camera', command: 'audit_mic_camera' },
    { id: 'browser-extensions', command: 'audit_browser_extensions' },
    { id: 'startup-programs', command: 'audit_startup_programs' },
    { id: 'scheduled-tasks', command: 'audit_scheduled_tasks' },
    { id: 'outbound-connections', command: 'audit_outbound_connections' },
    { id: 'hosts-file', command: 'audit_hosts_file' },
    { id: 'dev-secrets', command: 'audit_dev_secrets' },
    { id: 'unencrypted-pii', command: 'audit_unencrypted_pii' },
];

/** Count of un-acknowledged findings from a run, for the scheduler's
 *  notification. Mirrors what the old backend `run_scheduled_privacy_audit`
 *  returned, but derived from the same findings the page now shows. */
export interface PrivacyAuditSummary {
    totalUnacknowledged: number;
    high: number;
}

/** Auto-expand decision for a streamed-in scan: open it if it's unavailable or
 *  carries an un-acknowledged high/medium finding. */
function shouldAutoOpen(r: ScanResult, acks: string[]): boolean {
    return (
        r.unavailable ||
        r.findings.some(
            (f) => (f.severity === 'high' || f.severity === 'medium') && !acks.includes(f.id),
        )
    );
}

/**
 * Run the full Privacy Audit, streaming each scan's result into the module
 * stores above as it resolves. This is the SINGLE scan path: the page's "Scan"
 * button and the opt-in scheduler both call it, so a scheduled run populates the
 * exact same stores the page renders — open the page after a scheduled run and
 * the findings are already there (no empty first-run hero).
 *
 * Returns the un-acknowledged summary for the scheduler's notification. Guarded
 * by `scanning` so a manual scan and a scheduled run can't double-dispatch.
 */
export async function runPrivacyAuditScan(): Promise<PrivacyAuditSummary> {
    // A run is already in flight (e.g. the user clicked Scan while a scheduled
    // run fired) — don't start a second. Return an empty summary so the caller
    // doesn't notify off stale data; the in-flight run owns the stores.
    if (get(scanning)) return { totalUnacknowledged: 0, high: 0 };

    scanning.set(true);
    errorMsg.set(null);
    ranOnce.set(true);
    // Seed one placeholder card per scan, all marked loading; each scan then
    // fills ONLY its own entry as it resolves so fast scans render immediately.
    results.set(
        PRIVACY_SCANS.map((s) => ({
            scan: s.id,
            findings: [],
            summary: '',
            unavailable: false,
            loading: true,
        })),
    );
    openSections.set({});

    const acks = get(acknowledged);

    const applyResult = (id: string, value: ScanResult) => {
        results.update((rs) =>
            rs.map((r) => (r.scan === id ? { ...value, scan: id, loading: false } : r)),
        );
        openSections.update((o) => ({ ...o, [id]: shouldAutoOpen({ ...value, scan: id }, acks) }));
    };

    const tasks = PRIVACY_SCANS.map((s) =>
        invoke<ScanResult>(s.command)
            .then((value) => applyResult(s.id, value))
            .catch((reason) =>
                applyResult(s.id, {
                    scan: s.id,
                    findings: [],
                    summary: `Scan failed: ${reason}`,
                    unavailable: true,
                    loading: false,
                }),
            ),
    );

    try {
        await Promise.allSettled(tasks);
    } finally {
        scanning.set(false);
    }

    // Summarize for the scheduler: every un-acknowledged finding counts (matches
    // the old backend behavior); `high` is the Severity::High subset. Re-read
    // acks in case the user acknowledged something mid-scan.
    const finalAcks = get(acknowledged);
    let totalUnacknowledged = 0;
    let high = 0;
    for (const r of get(results)) {
        for (const f of r.findings) {
            if (finalAcks.includes(f.id)) continue;
            totalUnacknowledged += 1;
            if (f.severity === 'high') high += 1;
        }
    }
    return { totalUnacknowledged, high };
}
