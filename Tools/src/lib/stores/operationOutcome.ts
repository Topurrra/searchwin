/*
  Shared operation-outcome reporter.

  Almost every tool store ends its `run*` function with the same ~40-line
  block: branch on success / partial / failed / cancelled, then fire a
  notification-center entry (`notify`), a transient `toast`, and an
  operation-history entry (`recordActivity`). The three sinks each want a
  different "level" for the same outcome — e.g. a partial result is a
  `warning` notification, an `info` toast, but a `success` activity entry.

  That level-mapping was copy-pasted into every store, so a store that
  drifted (wrong toast level for a partial run, say) was a silent bug with
  no single place to fix it. `reportOperationOutcome` centralizes the
  mapping: a store passes its already-translated strings plus one `kind`,
  and the helper resolves the three levels and fires all three sinks.

  Stores still own their own i18n strings (titles/messages differ per tool
  and take per-tool interpolation values) — only the mechanical fan-out
  and level-mapping are shared.
*/

import { notify, type NotificationLevel } from './notifications';
import { toast, type ToastLevel } from './toasts';
import { recordActivity, type ActivityOutcome } from './activityLog';

/** The four terminal states a batch/tool operation can finish in.
 *  `partial` = some items succeeded and some failed in the same run. */
export type OperationOutcomeKind = 'success' | 'partial' | 'failed' | 'cancelled';

export interface OperationOutcomeReport {
    /** App-screen id (appScreens.ts) — routes the notification + activity
     *  entry back to the originating tool. */
    toolId: string;
    kind: OperationOutcomeKind;
    /** Notification-center (bell) entry. */
    notifyTitle: string;
    notifyMessage: string;
    /** Transient toast text. Falls back to `notifyMessage` when omitted —
     *  most tools want a shorter toast string, so they pass it explicitly. */
    toastMessage?: string;
    /** Operation-history (Settings → Activity) entry. */
    activitySummary: string;
    activityDetails?: string | null;
}

/** outcome kind → notification-center level. */
const NOTIFY_LEVEL: Record<OperationOutcomeKind, NotificationLevel> = {
    success: 'success',
    partial: 'warning',
    failed: 'error',
    cancelled: 'warning',
};

/** outcome kind → toast level. Note `partial`/`cancelled` are `info`, not
 *  `warning`/`error` — a toast is glanceable and shouldn't read as a hard
 *  failure when the run partly succeeded or the user cancelled it. */
const TOAST_LEVEL: Record<OperationOutcomeKind, ToastLevel> = {
    success: 'success',
    partial: 'info',
    failed: 'error',
    cancelled: 'info',
};

/** outcome kind → activity-log outcome. A `partial` run is logged as
 *  `success` because the operation did complete and produce output. */
const ACTIVITY_OUTCOME: Record<OperationOutcomeKind, ActivityOutcome> = {
    success: 'success',
    partial: 'success',
    failed: 'failed',
    cancelled: 'cancelled',
};

/**
 * Fire the notification + toast + activity-log entries for a finished
 * operation, with the per-sink level resolved from `kind`. Replaces the
 * hand-rolled three-call block at the end of every tool store's `run*`.
 */
export function reportOperationOutcome(report: OperationOutcomeReport): void {
    notify({
        level: NOTIFY_LEVEL[report.kind],
        title: report.notifyTitle,
        message: report.notifyMessage,
        toolId: report.toolId,
    });
    toast(report.toastMessage ?? report.notifyMessage, TOAST_LEVEL[report.kind]);
    // Fire-and-forget: recordActivity swallows its own errors, and a
    // logging hiccup must never look like the operation itself failed.
    void recordActivity({
        toolId: report.toolId,
        summary: report.activitySummary,
        details: report.activityDetails ?? null,
        outcome: ACTIVITY_OUTCOME[report.kind],
    });
}
