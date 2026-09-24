/*
  Central busy-tasks store.

  Any tool that runs a long-ish operation can post a busy entry; the
  StatusBar reads the store and shows a loader + label while at least
  one entry is active. The previous design hand-coded subscriptions to
  individual processing booleans in StatusBar — fine for 4 tools, untenable
  at 25+. This store gives every tool a single line to opt in, and keeps
  StatusBar to one subscription.

  Usage:
      const done = reportBusy('word-converter', 'Converting Word to PDF…');
      try {
          // ... do the work
      } finally {
          done();
      }

  The returned `done` is a clear-this-task closure — paired call style
  (vs explicit ID juggling) keeps the call sites tiny and prevents leaks
  if the tool forgets to clean up by passing the closure to a finally.
*/

import { writable, derived } from 'svelte/store';

export type BusyTask = {
    /** Stable identifier — unique per call. */
    id: string;
    /** App-screen id (matches appScreens.ts). Lets the StatusBar route
     * a click on the busy chip to the originating tool. */
    toolId: string;
    /** User-facing label, e.g. "Converting 12 docs to PDF". Keep it
     * short — the StatusBar has limited horizontal space. */
    label: string;
    /** When the task was reported. Used by the StatusBar to display
     * "Xs / Xm" elapsed if multiple tasks compete for one slot. */
    startedAtMs: number;
};

export const busyTasks = writable<BusyTask[]>([]);

/** True when at least one task is in flight. */
export const isAnyBusy = derived(busyTasks, ($tasks) => $tasks.length > 0);

/** The "currently most relevant" task to surface in the StatusBar. We
 * pick the OLDEST one because it's the user's first invocation and the
 * one they're probably waiting on; newer parallel tasks get summarized
 * via the "+N more" count. */
export const primaryBusyTask = derived(busyTasks, ($tasks) => {
    if ($tasks.length === 0) return null;
    let oldest = $tasks[0];
    for (const t of $tasks) {
        if (t.startedAtMs < oldest.startedAtMs) oldest = t;
    }
    return oldest;
});

let nextId = 0;

/** Register a new busy task and get a closure that clears it. ALWAYS
 * call the returned closure in a `finally` so a thrown error doesn't
 * leak the busy entry forever.
 *
 * If the same `toolId` reports a second task while the first is still
 * active, both are tracked separately — StatusBar shows the oldest in
 * the headline + "+1 more" suffix.
 */
export function reportBusy(toolId: string, label: string): () => void {
    nextId += 1;
    const id = `${toolId}-${nextId}-${Date.now()}`;
    const task: BusyTask = { id, toolId, label, startedAtMs: Date.now() };
    busyTasks.update((list) => [...list, task]);
    return () => {
        busyTasks.update((list) => list.filter((t) => t.id !== id));
    };
}

/** Defensive clear — removes any task with the given id. Normally
 * unused (the closure from reportBusy is the right way) but exposed
 * for stores that prefer to track ids themselves. */
export function clearBusy(id: string): void {
    busyTasks.update((list) => list.filter((t) => t.id !== id));
}
