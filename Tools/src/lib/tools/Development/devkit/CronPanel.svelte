<script lang="ts">
    /*
      Cron Builder — an offline crontab.guru + a real scheduler.

      • Build & Explain: type a cron line (5-field, or 6-field with seconds) or
        pick a preset, and KeepItLocal explains it in plain English, lists the
        real next runs, and lets you edit field by field. All local.
      • Schedule on this PC: translate the expression into native Windows Task
        Scheduler triggers and run a program/command on it — even when the app is
        closed, across reboots, with no background process of ours. We're honest
        when an expression can't be mapped natively (see cronEngine.planSchedule).
    */
    import { onMount } from 'svelte';
    import { Copy, CheckCircle2, AlertTriangle, CalendarClock, FolderOpen, Play, Trash2, Terminal } from '@lucide/svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { open as openDialog } from '@tauri-apps/plugin-dialog';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import { Button, ToolPanel, Checkbox } from '$lib/ui';
    import { parseCron, explainCron, nextRuns, planSchedule } from '$lib/stores/cronEngine';

    interface CronTaskInfo {
        id: string;
        state: string;
        action: string;
        nextRun: string;
        lastRun: string;
        lastResult: number;
    }

    let tab = $state<'build' | 'schedule'>('build');
    let raw = $state('*/15 * * * *');

    const presets: { name: string; expr: string }[] = [
        { name: 'Every 15 minutes', expr: '*/15 * * * *' },
        { name: 'Every hour', expr: '0 * * * *' },
        { name: 'Every day at midnight', expr: '0 0 * * *' },
        { name: 'Every day at 9am', expr: '0 9 * * *' },
        { name: 'Weekdays at 9am', expr: '0 9 * * MON-FRI' },
        { name: 'Twice daily (9am & 5pm)', expr: '0 9,17 * * *' },
        { name: 'Every Sunday at noon', expr: '0 12 * * SUN' },
        { name: '1st of the month', expr: '0 0 1 * *' },
        { name: 'Quarterly', expr: '0 0 1 JAN,APR,JUL,OCT *' },
        { name: 'Every 30 seconds', expr: '*/30 * * * * *' },
    ];
    const macros: string[] = ['@hourly', '@daily', '@weekly', '@monthly', '@yearly', '@reboot'];

    // ── Parse + derived views (shared by both tabs) ───────────────────────
    const result = $derived.by(() => {
        try {
            return { parsed: parseCron(raw), error: null as string | null };
        } catch (e) {
            return { parsed: null, error: e instanceof Error ? e.message : String(e) };
        }
    });
    const explanation = $derived(result.error ? result.error : explainCron(result.parsed!));
    const isReboot = $derived(!!result.parsed?.reboot);
    const runs = $derived.by(() => (result.parsed && !result.parsed.reboot ? nextRuns(result.parsed, new Date(), 5) : []));
    const schedulePlan = $derived(result.parsed ? planSchedule(result.parsed) : null);

    // ── Field editor ──────────────────────────────────────────────────────
    const parts = $derived(raw.trim().split(/\s+/));
    const hasSeconds = $derived(parts.length === 6);
    const showFields = $derived(!result.error && (parts.length === 5 || parts.length === 6));
    const fieldDefs = $derived(
        hasSeconds
            ? [
                  { label: 'Second', idx: 0, hint: '0-59' },
                  { label: 'Minute', idx: 1, hint: '0-59' },
                  { label: 'Hour', idx: 2, hint: '0-23' },
                  { label: 'Day-Month', idx: 3, hint: '1-31' },
                  { label: 'Month', idx: 4, hint: '1-12 / JAN' },
                  { label: 'Day-Week', idx: 5, hint: '0-6 / SUN' },
              ]
            : [
                  { label: 'Minute', idx: 0, hint: '0-59' },
                  { label: 'Hour', idx: 1, hint: '0-23' },
                  { label: 'Day-Month', idx: 2, hint: '1-31' },
                  { label: 'Month', idx: 3, hint: '1-12 / JAN' },
                  { label: 'Day-Week', idx: 4, hint: '0-6 / SUN' },
              ]
    );

    function setField(idx: number, value: string) {
        const p = raw.trim().split(/\s+/);
        p[idx] = value.trim() || '*';
        raw = p.join(' ');
    }
    function toggleSeconds() {
        const p = raw.trim().split(/\s+/);
        if (p.length === 6) raw = p.slice(1).join(' ');
        else if (p.length === 5) raw = '0 ' + p.join(' ');
    }

    function fmtRun(d: Date): string {
        return d.toLocaleString(undefined, {
            year: 'numeric',
            month: 'short',
            day: 'numeric',
            hour: '2-digit',
            minute: '2-digit',
            ...(hasSeconds ? { second: '2-digit' } : {}),
        });
    }
    function relativeFromNow(d: Date): string {
        const s = Math.round((d.getTime() - Date.now()) / 1000);
        if (s < 60) return `in ${s}s`;
        const m = Math.round(s / 60);
        if (m < 60) return `in ${m}m`;
        const h = Math.round(m / 60);
        if (h < 24) return `in ${h}h`;
        return `in ${Math.round(h / 24)}d`;
    }

    async function copyExpr() {
        await navigator.clipboard.writeText(raw);
        toast('Copied expression', 'success');
    }

    // ── Schedule on this PC ───────────────────────────────────────────────
    let runMode = $state<'command' | 'open'>('command');
    let command = $state('');
    let openPath = $state('');
    let creating = $state(false);
    let jobs = $state<CronTaskInfo[]>([]);
    let jobsLoaded = $state(false);

    async function loadJobs() {
        try {
            jobs = await invoke<CronTaskInfo[]>('list_cron_tasks');
        } catch {
            jobs = [];
        }
        jobsLoaded = true;
    }
    onMount(loadJobs);

    async function pickFile() {
        try {
            const res = await openDialog({ multiple: false, directory: false });
            if (typeof res === 'string') openPath = res;
        } catch (error) {
            errorToast("Couldn't open the file picker", error, { hint: 'Try again — Windows occasionally refuses dialog focus.' });
        }
    }

    /** Compose an Execute + Argument pair from the chosen run mode. */
    function buildAction(): { program: string; args: string } | null {
        if (runMode === 'command') {
            const c = command.trim();
            return c ? { program: 'cmd.exe', args: `/c ${c}` } : null;
        }
        const p = openPath.trim();
        // `start "" "<path>"` opens an exe, document, folder or URL with its
        // default handler — one path that covers every "open this" case.
        return p ? { program: 'cmd.exe', args: `/c start "" "${p}"` } : null;
    }
    const canCreate = $derived(!!schedulePlan?.ok && !!buildAction() && !creating);

    async function createSchedule() {
        if (!schedulePlan?.ok) return;
        const act = buildAction();
        if (!act) {
            toast('Choose what to run first', 'error');
            return;
        }
        creating = true;
        try {
            const id = `cron-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
            await invoke('create_cron_task', { id, program: act.program, args: act.args, workingDir: '', triggers: schedulePlan.triggers });
            toast('Schedule created', 'success');
            await loadJobs();
        } catch (error) {
            errorToast("Couldn't create the schedule", error, { hint: 'Check that the program path is correct and try again.' });
        } finally {
            creating = false;
        }
    }

    async function runNow(id: string) {
        try {
            await invoke('run_cron_task_now', { id });
            toast('Task started', 'success');
            setTimeout(loadJobs, 900);
        } catch (error) {
            errorToast("Couldn't start the task", error);
        }
    }
    async function deleteJob(id: string) {
        try {
            await invoke('delete_cron_task', { id });
            toast('Schedule deleted', 'success');
            await loadJobs();
        } catch (error) {
            errorToast("Couldn't delete the schedule", error);
        }
    }

    function fmtJobTime(iso: string): string {
        if (!iso) return '—';
        const d = new Date(iso);
        return isNaN(d.getTime()) ? '—' : d.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
    }
</script>

<div class="dt-panel">
    <div class="dt-head">
        <div class="dt-head-text">
            <h2 class="dt-title">Cron Builder — build, explain &amp; schedule</h2>
            <p class="dt-desc">Compose cron expressions field by field, get a plain-English explanation and the next run times, and schedule jobs. Runs locally.</p>
        </div>
        <div class="dt-seg" role="group" aria-label="Mode">
            <button class="dt-seg-btn" class:is-active={tab === 'build'} onclick={() => (tab = 'build')}>Build &amp; Explain</button>
            <button class="dt-seg-btn" class:is-active={tab === 'schedule'} onclick={() => (tab = 'schedule')}>Schedule on this PC</button>
        </div>
    </div>

    <!-- Expression bar (shared) -->
    <div class="dt-io">
        <div class="dt-io-head">
            <span class="dt-section-label">Expression</span>
            <span class="dt-cron-status {result.error ? 'dt-text-error' : 'dt-text-success'}">
                {#if result.error}
                    <AlertTriangle class="dt-ico" /> Invalid
                {:else}
                    <CheckCircle2 class="dt-ico" /> Valid
                {/if}
            </span>
        </div>
        <div class="dt-toolbar">
            <input
                id="cron-input"
                type="text"
                class="dt-input dt-mono dt-cron-input"
                class:is-error={result.error}
                bind:value={raw}
                spellcheck="false"
                aria-label="Cron expression"
            />
            <Button variant="primary" icon={Copy} onclick={copyExpr}>Copy</Button>
        </div>
    </div>

    <!-- Plain English (shared) -->
    <ToolPanel padding="md">
        <div class="dt-io-head">
            <span class="dt-section-label">Plain English</span>
        </div>
        <div class="dt-cron-explain {result.error ? 'dt-text-error' : ''}">{explanation}</div>
    </ToolPanel>

    {#if tab === 'build'}
        <!-- Field editor -->
        <ToolPanel padding="md">
            <div class="dt-col">
                {#if showFields}
                    <div class="dt-fields {hasSeconds ? 'cols-3' : 'cols-3'}">
                        {#each fieldDefs as f (f.idx)}
                            <label class="dt-field">
                                <span class="dt-label">{f.label}</span>
                                <input
                                    type="text"
                                    class="dt-input dt-mono dt-cron-field"
                                    value={parts[f.idx] ?? '*'}
                                    oninput={(e) => setField(f.idx, e.currentTarget.value)}
                                    spellcheck="false"
                                />
                                <span class="dt-hint dt-cron-field-hint">{f.hint}</span>
                            </label>
                        {/each}
                    </div>
                {/if}
                <Checkbox
                    checked={hasSeconds}
                    disabled={!!result.error && !showFields}
                    label="Include seconds (6-field)"
                    onchange={toggleSeconds}
                />
            </div>
        </ToolPanel>

        <!-- Presets -->
        <ToolPanel padding="md">
            <div class="dt-col">
                <div class="dt-io">
                    <div class="dt-io-head">
                        <span class="dt-section-label">Presets</span>
                    </div>
                    <div class="dt-chips">
                        {#each presets as p}
                            <Button variant="secondary" size="sm" title={p.expr} onclick={() => (raw = p.expr)}>{p.name}</Button>
                        {/each}
                    </div>
                </div>
                <div class="dt-io">
                    <div class="dt-io-head">
                        <span class="dt-section-label">Macros</span>
                    </div>
                    <div class="dt-chips">
                        {#each macros as m}
                            <Button variant="secondary" size="sm" onclick={() => (raw = m)}>{m}</Button>
                        {/each}
                    </div>
                </div>
            </div>
        </ToolPanel>

        <!-- Next runs -->
        {#if isReboot}
            <div class="dt-note">Runs once at system startup — no scheduled clock times.</div>
        {:else if runs.length > 0}
            <ToolPanel padding="md">
                <div class="dt-io-head">
                    <span class="dt-section-label">Next {runs.length} runs</span>
                </div>
                <div class="dt-cron-runs">
                    {#each runs as t, i}
                        <div class="dt-kv">
                            <span class="dt-kv-key">{i + 1}.</span>
                            <span class="dt-kv-val">{fmtRun(t)}</span>
                            <span class="dt-kv-note">{relativeFromNow(t)}</span>
                        </div>
                    {/each}
                </div>
            </ToolPanel>
        {:else if !result.error}
            <div class="dt-note">
                No upcoming runs in the next 8 years — this combination may never occur (e.g. February 30th).
            </div>
        {/if}

        <!-- Reference -->
        <details class="dt-cron-ref">
            <summary class="dt-cron-ref-summary">Cron reference</summary>
            <div class="dt-cron-ref-body dt-mono">
                <div><span class="dt-cron-tok">*</span> any value &nbsp;·&nbsp; <span class="dt-cron-tok">,</span> list (<code>1,5,10</code>) &nbsp;·&nbsp; <span class="dt-cron-tok">-</span> range (<code>1-5</code>) &nbsp;·&nbsp; <span class="dt-cron-tok">/</span> step (<code>*/15</code>)</div>
                <div>Names: <code>JAN-DEC</code>, <code>SUN-SAT</code> (or <code>0-6</code>, with <code>7</code> = Sunday)</div>
                <div>Macros: <code>@hourly @daily @weekly @monthly @yearly @reboot</code></div>
                <div class="dt-cron-ref-fmt">Format: <code>[second] minute hour day-of-month month day-of-week</code></div>
                <div class="dt-text-muted">When both day-of-month <em>and</em> day-of-week are set, a run fires when <em>either</em> matches (standard cron rule).</div>
            </div>
        </details>
    {:else}
        <!-- ───────────────── Schedule on this PC ───────────────── -->
        <!-- What to run -->
        <ToolPanel padding="md">
            <div class="dt-col">
                <div class="dt-io-head">
                    <span class="dt-section-label">What to run</span>
                </div>
                <div class="dt-seg" role="group" aria-label="What to run">
                    <button class="dt-seg-btn" class:is-active={runMode === 'command'} onclick={() => (runMode = 'command')}>
                        <Terminal class="dt-ico" /> Command
                    </button>
                    <button class="dt-seg-btn" class:is-active={runMode === 'open'} onclick={() => (runMode = 'open')}>
                        <FolderOpen class="dt-ico" /> Open file or app
                    </button>
                </div>

                {#if runMode === 'command'}
                    <input
                        type="text"
                        class="dt-input dt-mono"
                        bind:value={command}
                        spellcheck="false"
                        placeholder="e.g. python C:\scripts\backup.py"
                        aria-label="Command to run"
                    />
                    <p class="dt-hint">Runs via <code class="dt-mono">cmd /c</code> in your user account.</p>
                {:else}
                    <div class="dt-toolbar">
                        <input
                            type="text"
                            class="dt-input dt-mono dt-cron-grow"
                            bind:value={openPath}
                            spellcheck="false"
                            placeholder="Pick a program, document, or folder…"
                            aria-label="File or app to open"
                        />
                        <Button variant="secondary" icon={FolderOpen} onclick={pickFile}>Browse</Button>
                    </div>
                    <p class="dt-hint">Opens with its default app — exe, document, folder, or URL.</p>
                {/if}
            </div>
        </ToolPanel>

        <!-- Schedule plan -->
        <ToolPanel padding="md">
            <div class="dt-col">
                <div class="dt-io-head">
                    <span class="dt-section-label">Schedule plan</span>
                </div>
                {#if result.error}
                    <p class="dt-text-error dt-cron-plan-msg">Fix the expression first — it isn't valid.</p>
                {:else if schedulePlan?.ok}
                    <div class="dt-cron-plan">
                        {#each schedulePlan.summary as line}
                            <div class="dt-cron-plan-line"><CalendarClock class="dt-ico dt-text-success" /> {line}</div>
                        {/each}
                    </div>
                    {#each schedulePlan.warnings as w}
                        <p class="dt-text-warning dt-cron-plan-warn">{w}</p>
                    {/each}
                    <p class="dt-hint">Creates {schedulePlan.triggers.length} Windows Task Scheduler trigger{schedulePlan.triggers.length === 1 ? '' : 's'} — fires even when KeepItLocal is closed.</p>
                {:else}
                    <div class="dt-cron-plan-line dt-text-warning">
                        <AlertTriangle class="dt-ico" />
                        <span>{schedulePlan?.reason}</span>
                    </div>
                {/if}

                <div>
                    <Button
                        variant="primary"
                        icon={CalendarClock}
                        loading={creating}
                        disabled={!canCreate}
                        onclick={createSchedule}
                    >
                        {creating ? 'Creating…' : 'Create schedule'}
                    </Button>
                </div>
            </div>
        </ToolPanel>

        <!-- Existing schedules -->
        <ToolPanel padding="md">
            <div class="dt-col">
                <div class="dt-io-head">
                    <span class="dt-section-label">Your schedules</span>
                </div>
                {#if !jobsLoaded}
                    <p class="dt-hint">Loading…</p>
                {:else if jobs.length === 0}
                    <p class="dt-hint">No schedules yet. Build an expression and create one above.</p>
                {:else}
                    <div class="dt-list">
                        {#each jobs as job (job.id)}
                            <div class="dt-row">
                                <div class="dt-row-main">
                                    <span class="dt-row-name dt-mono" title={job.action}>{job.action || job.id}</span>
                                    <span class="dt-row-sub">
                                        Next: {fmtJobTime(job.nextRun)}{#if job.state} · <span class="dt-cron-state">{job.state}</span>{/if}
                                    </span>
                                </div>
                                <div class="dt-cron-row-actions">
                                    <Button variant="secondary" size="sm" icon={Play} onclick={() => runNow(job.id)} aria-label="Run now">Run</Button>
                                    <Button variant="danger" size="sm" icon={Trash2} onclick={() => deleteJob(job.id)} aria-label="Delete schedule">Delete</Button>
                                </div>
                            </div>
                        {/each}
                    </div>
                {/if}
            </div>
        </ToolPanel>
    {/if}
</div>

<style>
    /* Local-only, layout/token bits. dt-* classes stay global (owned by
       the parent shell) and are never redefined here. */
    .dt-panel :global(.dt-ico) {
        width: 14px;
        height: 14px;
        flex: none;
    }
    .dt-cron-status {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        font-size: 11px;
        font-weight: 500;
    }
    .dt-cron-input {
        flex: 1;
        min-width: 200px;
        height: 34px;
    }
    .dt-cron-input.is-error {
        border-color: color-mix(in srgb, var(--color-error) 55%, var(--color-border));
    }
    .dt-cron-explain {
        margin-top: 8px;
        font-size: 13px;
        line-height: 1.45;
        color: var(--color-text);
    }
    .dt-cron-field {
        text-align: center;
    }
    .dt-cron-field-hint {
        text-align: center;
    }
    .dt-chips {
        display: flex;
        flex-wrap: wrap;
        gap: 8px;
    }
    .dt-cron-runs {
        display: flex;
        flex-direction: column;
        gap: 4px;
        margin-top: 10px;
    }
    .dt-cron-grow {
        flex: 1;
        min-width: 200px;
    }
    .dt-cron-plan {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .dt-cron-plan-line {
        display: flex;
        align-items: center;
        gap: 8px;
        font-size: 13px;
        color: var(--color-text);
    }
    .dt-cron-plan-msg {
        font-size: 13px;
        margin: 0;
    }
    .dt-cron-plan-warn {
        margin: 4px 0 0;
        font-size: 11px;
    }
    .dt-cron-row-actions {
        flex: none;
        display: inline-flex;
        align-items: center;
        gap: 6px;
    }
    .dt-cron-state {
        text-transform: uppercase;
        letter-spacing: 0.03em;
    }
    .dt-cron-ref {
        padding: 12px 14px;
        font-size: 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel);
    }
    .dt-cron-ref-summary {
        cursor: pointer;
        color: var(--color-muted);
    }
    .dt-cron-ref-summary:hover {
        color: var(--color-text);
    }
    .dt-cron-ref-body {
        display: flex;
        flex-direction: column;
        gap: 8px;
        margin-top: 12px;
        padding: 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    .dt-cron-tok {
        color: var(--color-accent);
    }
    .dt-cron-ref-fmt {
        padding-top: 8px;
        border-top: 1px solid var(--color-border);
    }
</style>
