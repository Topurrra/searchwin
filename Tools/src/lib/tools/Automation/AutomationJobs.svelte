<script lang="ts">
    import { onMount } from 'svelte';
    import {
        automationJobStats,
        automationJobs,
        cancelAutomationJob,
        clearFinishedAutomationJobs,
        destroyAutomationJobsStoreListener,
        finishedAutomationJobs,
        initAutomationJobsStore,
        maxParallelAutomationJobs,
        queuedAutomationJobs,
        removeAutomationJob,
        rerunAutomationJob,
        runningAutomationJobs,
        type AutomationJob,
    } from '$lib/stores/automationJobs';
    import { CheckCircle2, Clock3, Layers3, Loader2, Play, RotateCcw, Trash2, XCircle } from '@lucide/svelte';

    onMount(() => {
        initAutomationJobsStore();
        return () => destroyAutomationJobsStoreListener();
    });

    function timeLabel(value: number | null) {
        if (!value) return '—';
        return new Date(value).toLocaleString([], { month: 'short', day: '2-digit', hour: '2-digit', minute: '2-digit' });
    }

    function duration(job: AutomationJob) {
        if (!job.started_at) return 'Not started';
        const end = job.finished_at ?? Date.now();
        const sec = Math.max(1, Math.round((end - job.started_at) / 1000));
        if (sec < 60) return `${sec}s`;
        return `${Math.floor(sec / 60)}m ${sec % 60}s`;
    }

    function statusClass(status: AutomationJob['status']) {
        if (status === 'completed') return 'text-success';
        if (status === 'failed') return 'text-error';
        if (status === 'cancelled' || status === 'cancelling') return 'text-warning';
        if (status === 'running') return 'text-accent';
        return 'text-muted';
    }

    function statusLabel(status: AutomationJob['status']) {
        if (status === 'queued') return 'Queued';
        if (status === 'running') return 'Running';
        if (status === 'cancelling') return 'Cancelling';
        if (status === 'completed') return 'Completed';
        if (status === 'failed') return 'Failed';
        return 'Cancelled';
    }
</script>

<section class="space-y-3">
    <div class="bg-panel border border-border rounded p-3">
        <div class="flex flex-wrap items-center justify-between gap-3">
            <div>
                <h2 class="text-sm font-semibold flex items-center gap-2"><Layers3 class="w-4 h-4 text-accent" /> Running Jobs</h2>
                <p class="text-xs text-muted mt-1">Automation V3.1 queue. Multiple safe jobs can run in parallel.</p>
            </div>

            <div class="flex flex-wrap items-center gap-2">
                <label class="flex items-center gap-2 text-xs text-muted">
                    Parallel jobs
                    <select bind:value={$maxParallelAutomationJobs} class="h-8 bg-panel-2 border border-border rounded px-2 text-sm text-text">
                        <option value={1}>1 safe</option>
                        <option value={2}>2 balanced</option>
                        <option value={3}>3 fast</option>
                        <option value={4}>4 max</option>
                    </select>
                </label>
                <button onclick={clearFinishedAutomationJobs} class="h-8 px-3 rounded bg-panel-2 border border-border text-sm hover:bg-bg flex items-center gap-1.5">
                    <Trash2 class="w-3.5 h-3.5" /> Clear finished
                </button>
            </div>
        </div>

        <div class="grid grid-cols-2 md:grid-cols-5 gap-2 mt-3 text-xs">
            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Queued</div><div class="font-semibold">{$automationJobStats.queued}</div></div>
            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Running</div><div class="font-semibold text-accent">{$automationJobStats.running}</div></div>
            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Done</div><div class="font-semibold text-success">{$automationJobStats.completed}</div></div>
            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Failed</div><div class="font-semibold text-error">{$automationJobStats.failed}</div></div>
            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Cancelled</div><div class="font-semibold text-warning">{$automationJobStats.cancelled}</div></div>
        </div>
    </div>

    {#if $runningAutomationJobs.length}
        <div class="space-y-2">
            <div class="text-xs font-medium text-muted uppercase tracking-wide">Running</div>
            {#each $runningAutomationJobs as job}
                <div class="bg-panel border border-border rounded p-3 space-y-2">
                    <div class="flex flex-wrap items-start justify-between gap-3">
                        <div class="min-w-0">
                            <div class="text-sm font-semibold truncate" title={job.title}>{job.title}</div>
                            <div class="text-xs text-muted truncate" title={job.current_file ?? job.message}>{job.current_file ?? job.message}</div>
                        </div>
                        <button onclick={() => cancelAutomationJob(job.id)} disabled={job.status === 'cancelling'} class="h-8 px-3 rounded bg-error-soft text-error border border-error-strong text-sm hover:bg-error-soft disabled:opacity-50">
                            {job.status === 'cancelling' ? 'Cancelling…' : 'Cancel'}
                        </button>
                    </div>
                    <div class="h-1.5 bg-bg rounded overflow-hidden">
                        <div class="h-full bg-accent transition-all" style="width: {Math.max(6, job.progress)}%"></div>
                    </div>
                    <div class="flex flex-wrap items-center justify-between gap-2 text-[11px] text-muted">
                        <span>{job.progress}% · {duration(job)}</span>
                        <span>{job.input_count} input file{job.input_count === 1 ? '' : 's'}</span>
                    </div>
                </div>
            {/each}
        </div>
    {/if}

    {#if $queuedAutomationJobs.length}
        <div class="space-y-2">
            <div class="text-xs font-medium text-muted uppercase tracking-wide">Queued</div>
            {#each $queuedAutomationJobs as job}
                <div class="bg-panel border border-border rounded p-3 flex flex-wrap items-center justify-between gap-3">
                    <div class="min-w-0">
                        <div class="text-sm font-semibold truncate flex items-center gap-2" title={job.title}><Clock3 class="w-3.5 h-3.5 text-muted" /> {job.title}</div>
                        <div class="text-xs text-muted truncate" title={job.message}>{job.input_count} file{job.input_count === 1 ? '' : 's'} · {job.message}</div>
                    </div>
                    <button onclick={() => cancelAutomationJob(job.id)} class="h-8 px-3 rounded bg-panel-2 border border-border text-sm hover:bg-bg">Remove</button>
                </div>
            {/each}
        </div>
    {/if}

    <div class="space-y-2">
        <div class="text-xs font-medium text-muted uppercase tracking-wide">Recent finished</div>
        {#if $finishedAutomationJobs.length}
            <div class="space-y-2 max-h-[420px] overflow-auto pr-1">
                {#each $finishedAutomationJobs as job}
                    <div class="bg-panel border border-border rounded p-3">
                        <div class="flex flex-wrap items-start justify-between gap-3">
                            <div class="min-w-0">
                                <div class="text-sm font-semibold truncate flex items-center gap-2">
                                    {#if job.status === 'completed'}
                                        <CheckCircle2 class="w-4 h-4 text-success" />
                                    {:else if job.status === 'failed'}
                                        <XCircle class="w-4 h-4 text-error" />
                                    {:else}
                                        <Clock3 class="w-4 h-4 text-warning" />
                                    {/if}
                                    {job.title}
                                </div>
                                <div class="text-xs text-muted truncate" title={job.message}>{job.message}</div>
                            </div>
                            <div class="flex items-center gap-1">
                                <button onclick={() => rerunAutomationJob(job.id)} class="h-8 px-2 rounded bg-panel-2 border border-border text-xs hover:bg-bg flex items-center gap-1"><RotateCcw class="w-3.5 h-3.5" /> Run again</button>
                                <button onclick={() => removeAutomationJob(job.id)} class="h-8 px-2 rounded bg-panel-2 border border-border text-xs hover:bg-bg"><Trash2 class="w-3.5 h-3.5" /></button>
                            </div>
                        </div>
                        <div class="grid grid-cols-2 md:grid-cols-5 gap-2 mt-2 text-[11px]">
                            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Status</div><div class={statusClass(job.status)}>{statusLabel(job.status)}</div></div>
                            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Success</div><div class="text-success">{job.success_count}</div></div>
                            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Failed</div><div class="text-error">{job.failed_count}</div></div>
                            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Duration</div><div>{duration(job)}</div></div>
                            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Finished</div><div>{timeLabel(job.finished_at)}</div></div>
                        </div>
                    </div>
                {/each}
            </div>
        {:else}
            <div class="bg-panel border border-dashed border-border rounded p-8 text-center text-sm text-muted">
                <Play class="w-6 h-6 mx-auto mb-2 opacity-60" />
                No automation jobs yet. Queue a recipe from Setup.
            </div>
        {/if}
    </div>
</section>
