import { describe, expect, it, beforeEach, vi } from 'vitest';
import { get } from 'svelte/store';

// recordActivity (activityLog) and recordLog (errorLog, reached via the
// toast pipeline) both call the Tauri `invoke` bridge, which doesn't exist
// outside the desktop runtime. Mock it so the helper's three sinks can be
// exercised in a plain test process.
const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

import { reportOperationOutcome, type OperationOutcomeKind } from './operationOutcome';
import { notifications } from './notifications';
import { toasts } from './toasts';

describe('reportOperationOutcome', () => {
    beforeEach(() => {
        invokeMock.mockReset();
        invokeMock.mockResolvedValue(undefined);
        notifications.set([]);
        toasts.set([]);
    });

    // The per-sink level mapping is the logic that was copy-pasted into
    // every store and could silently drift — pin all four outcomes.
    const cases: {
        kind: OperationOutcomeKind;
        notify: string;
        toast: string;
        activity: string;
    }[] = [
        { kind: 'success', notify: 'success', toast: 'success', activity: 'success' },
        { kind: 'partial', notify: 'warning', toast: 'info', activity: 'success' },
        { kind: 'failed', notify: 'error', toast: 'error', activity: 'failed' },
        { kind: 'cancelled', notify: 'warning', toast: 'info', activity: 'cancelled' },
    ];

    for (const c of cases) {
        it(`maps the '${c.kind}' outcome to the right per-sink levels`, () => {
            reportOperationOutcome({
                toolId: 'demo-tool',
                kind: c.kind,
                notifyTitle: 'Title',
                notifyMessage: 'Message',
                toastMessage: 'Toast',
                activitySummary: 'Summary',
                activityDetails: 'Details',
            });

            const latestNotification = get(notifications)[0];
            expect(latestNotification.level).toBe(c.notify);
            expect(latestNotification.title).toBe('Title');
            expect(latestNotification.toolId).toBe('demo-tool');

            const latestToast = get(toasts).at(-1);
            expect(latestToast?.level).toBe(c.toast);
            expect(latestToast?.message).toBe('Toast');

            const activityCall = invokeMock.mock.calls.find((call) => call[0] === 'record_activity');
            expect(activityCall).toBeDefined();
            expect(activityCall?.[1].input.outcome).toBe(c.activity);
            expect(activityCall?.[1].input.toolId).toBe('demo-tool');
        });
    }

    it('falls back to notifyMessage when toastMessage is omitted', () => {
        reportOperationOutcome({
            toolId: 'demo-tool',
            kind: 'success',
            notifyTitle: 'Title',
            notifyMessage: 'Fallback message',
            activitySummary: 'Summary',
        });
        expect(get(toasts).at(-1)?.message).toBe('Fallback message');
    });

    it('passes null activity details through when omitted', () => {
        reportOperationOutcome({
            toolId: 'demo-tool',
            kind: 'failed',
            notifyTitle: 'Title',
            notifyMessage: 'Message',
            activitySummary: 'Summary',
        });
        const activityCall = invokeMock.mock.calls.find((call) => call[0] === 'record_activity');
        expect(activityCall?.[1].input.details).toBeNull();
    });
});
