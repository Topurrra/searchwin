import { derived, get, writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { notify } from './notifications';
import { toast } from './toasts';
import {
    cancelImageOperation,
    fmtBytes,
    newOperationId,
    setBusy,
    type ImageProgress,
    type ImageResult,
} from './imageBatchCommon';
import {
    automationCompressEnabled,
    automationCompressQuality,
    automationConvertEnabled,
    automationConvertFormat,
    automationFiles,
    automationImageQuality,
    automationOutputDir,
    automationRecipeName,
    automationResizeEnabled,
    automationResizeLongestSide,
    automationStripMetadataEnabled,
    enabledAutomationSteps,
    selectedAutomationRecipe,
    automationActivity,
    type AutomationActivityItem,
    type AutomationImageFormat,
    type AutomationRecipeId,
} from './automationRecipes';

export type AutomationJobStatus = 'queued' | 'running' | 'cancelling' | 'completed' | 'failed' | 'cancelled';
export type AutomationJobWeight = 'normal' | 'heavy' | 'exclusive';

export type ImageAutomationJobOptions = {
    paths: string[];
    output_dir: string | null;
    suffix: string;
    recipe_id: AutomationRecipeId;
    resize_enabled: boolean;
    resize_longest_side: number;
    convert_enabled: boolean;
    output_format: AutomationImageFormat;
    compress_enabled: boolean;
    compress_quality: number;
    image_quality: number;
    strip_metadata: boolean;
};

export type AutomationJob = {
    id: string;
    operation_id: string;
    title: string;
    kind: 'image';
    weight: AutomationJobWeight;
    status: AutomationJobStatus;
    created_at: number;
    started_at: number | null;
    finished_at: number | null;
    input_count: number;
    success_count: number;
    failed_count: number;
    cancelled_count: number;
    progress: number;
    current_file: string | null;
    message: string;
    output_dir: string | null;
    options: ImageAutomationJobOptions;
    results: ImageResult[];
    error: string | null;
};

export const automationJobs = writable<AutomationJob[]>([]);
export const automationJobProgress = writable<Record<string, ImageProgress>>({});
export const maxParallelAutomationJobs = writable(2);
export const automationJobsInitialized = writable(false);

let unlisten: UnlistenFn | null = null;
let schedulerRunning = false;

export const queuedAutomationJobs = derived(automationJobs, ($jobs) => $jobs.filter((job) => job.status === 'queued'));
export const runningAutomationJobs = derived(automationJobs, ($jobs) => $jobs.filter((job) => job.status === 'running' || job.status === 'cancelling'));
export const finishedAutomationJobs = derived(automationJobs, ($jobs) => $jobs.filter((job) => job.status === 'completed' || job.status === 'failed' || job.status === 'cancelled'));
export const automationJobStats = derived(automationJobs, ($jobs) => ({
    queued: $jobs.filter((job) => job.status === 'queued').length,
    running: $jobs.filter((job) => job.status === 'running' || job.status === 'cancelling').length,
    completed: $jobs.filter((job) => job.status === 'completed').length,
    failed: $jobs.filter((job) => job.status === 'failed').length,
    cancelled: $jobs.filter((job) => job.status === 'cancelled').length,
}));

export async function initAutomationJobsStore() {
    if (get(automationJobsInitialized)) return;
    automationJobsInitialized.set(true);

    unlisten = await listen<ImageProgress>('image-progress', (event) => {
        const progress = event.payload;
        updateJobProgress(progress);
    });
}

export function destroyAutomationJobsStoreListener() {
    unlisten?.();
    unlisten = null;
    automationJobsInitialized.set(false);
}

function updateJobProgress(progress: ImageProgress) {
    const jobs = get(automationJobs);
    const job = jobs.find((item) => item.operation_id === progress.operation_id);
    if (!job) return;

    automationJobProgress.update((current) => ({ ...current, [`${progress.operation_id}:${progress.source_path}`]: progress }));

    const fileProgressItems = Object.values(get(automationJobProgress)).filter((item) => item.operation_id === progress.operation_id);
    const aggregate = fileProgressItems.length
        ? Math.round(fileProgressItems.reduce((sum, item) => sum + item.progress, 0) / fileProgressItems.length)
        : progress.progress;

    automationJobs.update((current) => current.map((item) => {
        if (item.operation_id !== progress.operation_id) return item;
        return {
            ...item,
            progress: Math.min(99, Math.max(item.progress, aggregate)),
            current_file: progress.file_name,
            message: progress.message ?? progress.stage,
        };
    }));
}

function imageResultStats(results: ImageResult[]) {
    const success = results.filter((r) => r.success).length;
    const cancelled = results.filter((r) => r.error === 'Cancelled').length;
    const failed = results.filter((r) => !r.success && r.error !== 'Cancelled').length;
    return { success, failed, cancelled };
}

function makeId(prefix: string) {
    return globalThis.crypto?.randomUUID?.() ?? `${prefix}_${Date.now()}_${Math.random().toString(16).slice(2)}`;
}

function hasExclusiveRunning(jobs: AutomationJob[]) {
    return jobs.some((job) => (job.status === 'running' || job.status === 'cancelling') && job.weight === 'exclusive');
}

function canStartJob(job: AutomationJob, jobs: AutomationJob[]) {
    const running = jobs.filter((item) => item.status === 'running' || item.status === 'cancelling');
    if (running.length >= get(maxParallelAutomationJobs)) return false;
    if (hasExclusiveRunning(jobs)) return false;
    if (job.weight === 'exclusive' && running.length > 0) return false;
    return true;
}

async function syncBusyState() {
    const anyRunning = get(automationJobs).some((job) => job.status === 'running' || job.status === 'cancelling');
    await setBusy(anyRunning);
}

export function enqueueCurrentAutomationRecipe() {
    const files = get(automationFiles);
    if (!files.length) {
        toast('Choose images first', 'error');
        return;
    }

    const steps = get(enabledAutomationSteps);
    if (!steps.length) {
        toast('Choose at least one action', 'error');
        return;
    }

    const title = get(automationRecipeName).trim() || 'Image automation';
    const operationId = newOperationId('automation-image');
    const job: AutomationJob = {
        id: makeId('job'),
        operation_id: operationId,
        title,
        kind: 'image',
        weight: 'normal',
        status: 'queued',
        created_at: Date.now(),
        started_at: null,
        finished_at: null,
        input_count: files.length,
        success_count: 0,
        failed_count: 0,
        cancelled_count: 0,
        progress: 0,
        current_file: null,
        message: 'Waiting for an automation worker',
        output_dir: get(automationOutputDir),
        options: {
            paths: [...files],
            output_dir: get(automationOutputDir),
            suffix: '_automated',
            recipe_id: get(selectedAutomationRecipe),
            resize_enabled: get(automationResizeEnabled),
            resize_longest_side: get(automationResizeLongestSide),
            convert_enabled: get(automationConvertEnabled),
            output_format: get(automationConvertFormat),
            compress_enabled: get(automationCompressEnabled),
            compress_quality: get(automationCompressQuality),
            image_quality: get(automationImageQuality),
            strip_metadata: get(automationStripMetadataEnabled),
        },
        results: [],
        error: null,
    };

    automationJobs.update((jobs) => [job, ...jobs].slice(0, 100));
    toast(`Queued: ${title}`, 'success');
    notify({ level: 'info', title: 'Automation queued', message: `${files.length} file${files.length === 1 ? '' : 's'} waiting`, toolId: 'automation-recipes' });
    void scheduleAutomationWorkers();
}

async function scheduleAutomationWorkers() {
    if (schedulerRunning) return;
    schedulerRunning = true;

    try {
        while (true) {
            const jobs = get(automationJobs);
            const next = jobs.find((job) => job.status === 'queued' && canStartJob(job, jobs));
            if (!next) break;
            void runAutomationJob(next.id);
            await new Promise((resolve) => setTimeout(resolve, 40));
        }
    } finally {
        schedulerRunning = false;
    }
}

async function runAutomationJob(jobId: string) {
    const job = get(automationJobs).find((item) => item.id === jobId);
    if (!job || job.status !== 'queued') return;

    const startedAt = Date.now();
    automationJobs.update((jobs) => jobs.map((item) => item.id === jobId ? {
        ...item,
        status: 'running',
        started_at: startedAt,
        message: 'Starting automation',
        progress: 3,
    } : item));
    await syncBusyState();

    try {
        const results = await invoke<ImageResult[]>('run_image_automation', {
            options: {
                operation_id: job.operation_id,
                paths: job.options.paths,
                output_dir: job.options.output_dir,
                suffix: job.options.suffix,
                resize_enabled: job.options.resize_enabled,
                resize_longest_side: job.options.resize_longest_side,
                convert_enabled: job.options.convert_enabled,
                output_format: job.options.output_format,
                compress_enabled: job.options.compress_enabled,
                compress_quality: job.options.compress_quality,
                image_quality: job.options.image_quality,
                strip_metadata: job.options.strip_metadata,
            },
        });

        const stats = imageResultStats(results);
        const requestedCancel = get(automationJobs).find((item) => item.id === jobId)?.status === 'cancelling';
        const status: AutomationJobStatus = requestedCancel || stats.cancelled > 0
            ? 'cancelled'
            : stats.failed > 0 && stats.success === 0
                ? 'failed'
                : 'completed';
        const finishedAt = Date.now();

        automationJobs.update((jobs) => jobs.map((item) => item.id === jobId ? {
            ...item,
            status,
            finished_at: finishedAt,
            progress: 100,
            success_count: stats.success,
            failed_count: stats.failed,
            cancelled_count: stats.cancelled,
            current_file: null,
            message: status === 'cancelled'
                ? `${stats.success} output${stats.success === 1 ? '' : 's'} created before cancellation`
                : status === 'failed'
                    ? 'No output files were created'
                    : `${stats.success} output file${stats.success === 1 ? '' : 's'} created`,
            results,
            error: null,
        } : item));

        await addJobActivity(job, status, startedAt, finishedAt, stats);

        if (status === 'completed') {
            notify({ level: stats.failed > 0 ? 'warning' : 'success', title: stats.failed > 0 ? 'Automation finished with warnings' : 'Automation finished', message: `${stats.success} output${stats.success === 1 ? '' : 's'} created`, toolId: 'automation-recipes' });
        } else if (status === 'cancelled') {
            notify({ level: 'warning', title: 'Automation cancelled', message: `${stats.success} output${stats.success === 1 ? '' : 's'} created before cancellation`, toolId: 'automation-recipes' });
        } else {
            notify({ level: 'error', title: 'Automation failed', message: job.title, toolId: 'automation-recipes' });
        }
    } catch (error) {
        const message = String(error);
        const requestedCancel = get(automationJobs).find((item) => item.id === jobId)?.status === 'cancelling';
        const finishedAt = Date.now();
        const status: AutomationJobStatus = requestedCancel ? 'cancelled' : 'failed';

        automationJobs.update((jobs) => jobs.map((item) => item.id === jobId ? {
            ...item,
            status,
            finished_at: finishedAt,
            progress: status === 'cancelled' ? item.progress : 100,
            current_file: null,
            message,
            error: message,
        } : item));

        await addJobActivity(job, status, startedAt, finishedAt, { success: 0, failed: requestedCancel ? 0 : job.input_count, cancelled: requestedCancel ? job.input_count : 0 });
        notify({ level: requestedCancel ? 'warning' : 'error', title: requestedCancel ? 'Automation cancelled' : 'Automation failed', message, toolId: 'automation-recipes' });
    } finally {
        await syncBusyState();
        void scheduleAutomationWorkers();
    }
}

async function addJobActivity(job: AutomationJob, status: AutomationJobStatus, startedAt: number, finishedAt: number, stats: { success: number; failed: number; cancelled: number }) {
    const item: AutomationActivityItem = {
        id: makeId('activity'),
        recipe_name: job.title,
        level: status === 'completed' ? (stats.failed > 0 ? 'warning' : 'success') : status === 'cancelled' ? 'warning' : 'error',
        started_at: startedAt,
        finished_at: finishedAt,
        input_count: job.input_count,
        success_count: stats.success,
        failed_count: stats.failed,
        cancelled: status === 'cancelled',
        output_dir: job.output_dir,
        message: status === 'cancelled'
            ? `${stats.success} output${stats.success === 1 ? '' : 's'} created before cancellation.`
            : status === 'failed'
                ? 'Automation failed.'
                : `${stats.success} output${stats.success === 1 ? '' : 's'} created${stats.failed ? `, ${stats.failed} failed` : ''}.`,
    };

    automationActivity.update((items) => [item, ...items].slice(0, 80));
    try {
        const saved = await invoke<AutomationActivityItem[]>('add_automation_activity', { item });
        automationActivity.set(saved);
    } catch (error) {
        toast(`Could not save automation activity: ${String(error)}`, 'error');
    }
}

export async function cancelAutomationJob(jobId: string) {
    const job = get(automationJobs).find((item) => item.id === jobId);
    if (!job) return;

    if (job.status === 'queued') {
        automationJobs.update((jobs) => jobs.map((item) => item.id === jobId ? {
            ...item,
            status: 'cancelled',
            finished_at: Date.now(),
            message: 'Cancelled before it started',
            progress: 100,
        } : item));
        void scheduleAutomationWorkers();
        return;
    }

    if (job.status !== 'running') return;
    automationJobs.update((jobs) => jobs.map((item) => item.id === jobId ? { ...item, status: 'cancelling', message: 'Cancelling…' } : item));
    await cancelImageOperation(job.operation_id);
}

export function clearFinishedAutomationJobs() {
    automationJobs.update((jobs) => jobs.filter((job) => job.status === 'queued' || job.status === 'running' || job.status === 'cancelling'));
}

export function removeAutomationJob(jobId: string) {
    const job = get(automationJobs).find((item) => item.id === jobId);
    if (!job || job.status === 'running' || job.status === 'cancelling') return;
    automationJobs.update((jobs) => jobs.filter((item) => item.id !== jobId));
}

export function rerunAutomationJob(jobId: string) {
    const job = get(automationJobs).find((item) => item.id === jobId);
    if (!job || job.status === 'running' || job.status === 'cancelling') return;
    const operationId = newOperationId('automation-image');
    const clone: AutomationJob = {
        ...job,
        id: makeId('job'),
        operation_id: operationId,
        status: 'queued',
        created_at: Date.now(),
        started_at: null,
        finished_at: null,
        success_count: 0,
        failed_count: 0,
        cancelled_count: 0,
        progress: 0,
        current_file: null,
        message: 'Waiting for an automation worker',
        results: [],
        error: null,
    };
    automationJobs.update((jobs) => [clone, ...jobs].slice(0, 100));
    toast(`Queued again: ${clone.title}`, 'success');
    void scheduleAutomationWorkers();
}

export function fmtJobBytes(value: number) {
    return fmtBytes(value);
}
