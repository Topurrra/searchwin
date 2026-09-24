/*
  Thin client for the operation-history audit log.

  Tools call `recordActivity({ toolId, summary, details, outcome })`
  after their action completes (success/cancelled/failed). The Settings
  → Activity panel reads the log via `listActivity` and can wipe it via
  `clearActivity`.

  Failures of the log writes are swallowed — the tool's primary
  operation already succeeded; we don't want a logging hiccup to look
  like the operation failed.
*/

import { invoke } from '@tauri-apps/api/core';

export type ActivityOutcome = 'success' | 'cancelled' | 'failed';

export type ActivityEntry = {
    id: number;
    timestampMs: number;
    toolId: string;
    summary: string;
    details: string | null;
    outcome: ActivityOutcome | string;
};

export type RecordActivityInput = {
    toolId: string;
    summary: string;
    details?: string | null;
    outcome: ActivityOutcome;
};

export async function recordActivity(input: RecordActivityInput): Promise<void> {
    try {
        await invoke('record_activity', {
            input: {
                toolId: input.toolId,
                summary: input.summary,
                details: input.details ?? null,
                outcome: input.outcome,
            },
        });
    } catch (error) {
        // Best-effort. Log to console so it shows up in dev tools but
        // never bubble back to the tool's UI.
        console.warn('activity log write failed:', error);
    }
}

export async function listActivity(): Promise<ActivityEntry[]> {
    return invoke<ActivityEntry[]>('list_activity');
}

export async function clearActivity(): Promise<void> {
    await invoke('clear_activity');
}
