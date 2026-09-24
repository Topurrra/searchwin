import { derived, get, writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { toast } from './toasts';
import { reportBusy } from './globalBusy';
import { reportOperationOutcome } from './operationOutcome';
import {
    addUnique,
    cancelImageOperation,
    fileName,
    fmtBytes,
    newOperationId,
    setBusy,
    type ImageProgress,
    type ImageResult,
} from './imageBatchCommon';

export type AutomationRecipeId = 'clean-photos' | 'web-ready' | 'email-small' | 'safe-share-images' | 'custom-image';
export type AutomationStepId = 'resize' | 'convert' | 'compress' | 'strip_metadata';
export type AutomationImageFormat = 'jpg' | 'png' | 'webp' | 'bmp' | 'tiff' | 'ico' | 'gif';
export type AutomationView = 'hub' | 'setup' | 'jobs' | 'activity';

export type BuiltInAutomationRecipe = {
    id: AutomationRecipeId;
    title: string;
    badge: string;
    short: string;
    safeLine: string;
    icon: 'shield' | 'image' | 'mail' | 'package' | 'settings';
};

export type SavedAutomationRecipe = {
    id: string;
    name: string;
    recipe_id: AutomationRecipeId;
    resize_enabled: boolean;
    resize_longest_side: number;
    convert_enabled: boolean;
    convert_format: AutomationImageFormat;
    compress_enabled: boolean;
    compress_quality: number;
    image_quality: number;
    strip_metadata_enabled: boolean;
    created_at: number;
};

export type AutomationActivityItem = {
    id: string;
    recipe_name: string;
    level: 'success' | 'warning' | 'error' | 'info';
    started_at: number;
    finished_at: number;
    input_count: number;
    success_count: number;
    failed_count: number;
    cancelled: boolean;
    output_dir: string | null;
    message: string;
};

const SUPPORTED_IMAGE_EXTS = new Set(['jpg', 'jpeg', 'png', 'webp', 'bmp', 'tiff', 'tif', 'gif', 'svg', 'ico']);

export const builtInAutomationRecipes: BuiltInAutomationRecipe[] = [
    {
        id: 'clean-photos',
        title: 'Clean Photos',
        badge: 'Privacy',
        short: 'Create image copies with hidden metadata removed where supported.',
        safeLine: 'Best before sharing photos online or sending them to someone.',
        icon: 'shield',
    },
    {
        id: 'web-ready',
        title: 'Web-Ready Images',
        badge: 'Images',
        short: 'Resize large images, convert to WebP, and compress them.',
        safeLine: 'Good for websites, blogs, shops, and smaller uploads.',
        icon: 'image',
    },
    {
        id: 'email-small',
        title: 'Email-Friendly Images',
        badge: 'Small files',
        short: 'Make lighter image copies that are easier to email or message.',
        safeLine: 'Keeps things simple: smaller files, originals unchanged.',
        icon: 'mail',
    },
    {
        id: 'safe-share-images',
        title: 'Safe Share Images',
        badge: 'Share',
        short: 'Resize, compress, and clean image copies for safer sharing.',
        safeLine: 'Creates one clean final output per image.',
        icon: 'package',
    },
    {
        id: 'custom-image',
        title: 'Custom Image Recipe',
        badge: 'Advanced',
        short: 'Choose image actions manually, then review before running.',
        safeLine: 'For users who want more control without writing scripts.',
        icon: 'settings',
    },
];

export const automationView = writable<AutomationView>('hub');
export const selectedAutomationRecipe = writable<AutomationRecipeId>('web-ready');
export const automationFiles = writable<string[]>([]);
export const automationOutputDir = writable<string | null>(null);
export const automationRecipeName = writable('Web-ready images');

export const automationResizeEnabled = writable(true);
export const automationResizeLongestSide = writable(1600);
export const automationConvertEnabled = writable(true);
export const automationConvertFormat = writable<AutomationImageFormat>('webp');
export const automationCompressEnabled = writable(true);
export const automationCompressQuality = writable(85);
export const automationImageQuality = writable(88);
export const automationStripMetadataEnabled = writable(false);

export const automationProcessing = writable(false);
export const automationCancelling = writable(false);
export const automationOperationId = writable('');
export const automationCurrentStep = writable<AutomationStepId | null>(null);
export const automationProgressByKey = writable<Record<string, ImageProgress>>({});
export const automationFinalResults = writable<ImageResult[]>([]);
export const automationLastError = writable<string | null>(null);
export const automationRunStartedAt = writable<number | null>(null);

export const savedAutomationRecipes = writable<SavedAutomationRecipe[]>([]);
export const automationActivity = writable<AutomationActivityItem[]>([]);
export const automationPersistenceReady = writable(false);
export const automationPersistenceError = writable<string | null>(null);

let initialized = false;
let unlisten: UnlistenFn | null = null;

async function loadPersistentAutomationData() {
    try {
        const [recipes, activity] = await Promise.all([
            invoke<SavedAutomationRecipe[]>('get_automation_recipes'),
            invoke<AutomationActivityItem[]>('get_automation_activity'),
        ]);
        savedAutomationRecipes.set(recipes);
        automationActivity.set(activity);
        automationPersistenceReady.set(true);
        automationPersistenceError.set(null);
    } catch (error) {
        const message = String(error);
        automationPersistenceError.set(message);
        automationPersistenceReady.set(false);
        toast(`Automation storage unavailable: ${message}`, 'error');
    }
}

async function persistSavedRecipes(recipes: SavedAutomationRecipe[]) {
    try {
        const saved = await invoke<SavedAutomationRecipe[]>('save_automation_recipes', { recipes });
        savedAutomationRecipes.set(saved);
        automationPersistenceError.set(null);
    } catch (error) {
        const message = String(error);
        automationPersistenceError.set(message);
        toast(`Could not save automation recipes: ${message}`, 'error');
    }
}

export async function refreshAutomationPersistence() {
    await loadPersistentAutomationData();
}

export async function initAutomationRecipesStore() {
    if (initialized) return;
    initialized = true;

    await loadPersistentAutomationData();

    unlisten = await listen<ImageProgress>('image-progress', (event) => {
        const progress = event.payload;
        if (progress.operation_id !== get(automationOperationId)) return;
        const key = `${progress.tool}:${progress.source_path}`;
        automationCurrentStep.set(progress.tool as AutomationStepId);
        automationProgressByKey.update((current) => ({ ...current, [key]: progress }));
    });
}

export function destroyAutomationRecipesStoreListener() {
    unlisten?.();
    unlisten = null;
    initialized = false;
}

export function isSupportedAutomationImage(path: string) {
    const ext = path.split('.').pop()?.toLowerCase() ?? '';
    return SUPPORTED_IMAGE_EXTS.has(ext);
}

export function addAutomationFiles(paths: string[]) {
    const accepted = paths.filter(isSupportedAutomationImage);
    const rejected = paths.length - accepted.length;
    automationFiles.update((current) => addUnique(current, accepted));
    if (rejected > 0) toast(`${rejected} unsupported file${rejected === 1 ? '' : 's'} skipped`, 'info');
}

export function removeAutomationFile(path: string) {
    if (get(automationProcessing)) return;
    automationFiles.update((current) => current.filter((item) => item !== path));
}

export function clearAutomationFiles() {
    if (get(automationProcessing)) return;
    automationFiles.set([]);
    automationFinalResults.set([]);
    automationProgressByKey.set({});
    automationLastError.set(null);
}

export function clearAutomationRun() {
    if (get(automationProcessing)) return;
    automationFinalResults.set([]);
    automationProgressByKey.set({});
    automationLastError.set(null);
    automationOperationId.set('');
    automationCancelling.set(false);
    automationCurrentStep.set(null);
    automationRunStartedAt.set(null);
}

export function selectBuiltInRecipe(recipeId: AutomationRecipeId) {
    if (get(automationProcessing)) return;
    selectedAutomationRecipe.set(recipeId);
    applyRecipeDefaults(recipeId);
    automationView.set('setup');
}

export function applyRecipeDefaults(recipeId: AutomationRecipeId) {
    selectedAutomationRecipe.set(recipeId);

    if (recipeId === 'clean-photos') {
        automationRecipeName.set('Clean Photos');
        automationResizeEnabled.set(false);
        automationResizeLongestSide.set(1600);
        automationConvertEnabled.set(false);
        automationConvertFormat.set('jpg');
        automationCompressEnabled.set(false);
        automationCompressQuality.set(90);
        automationImageQuality.set(92);
        automationStripMetadataEnabled.set(true);
    } else if (recipeId === 'web-ready') {
        automationRecipeName.set('Web-ready images');
        automationResizeEnabled.set(true);
        automationResizeLongestSide.set(1600);
        automationConvertEnabled.set(true);
        automationConvertFormat.set('webp');
        automationCompressEnabled.set(true);
        automationCompressQuality.set(85);
        automationImageQuality.set(88);
        automationStripMetadataEnabled.set(false);
    } else if (recipeId === 'email-small') {
        automationRecipeName.set('Email-friendly images');
        automationResizeEnabled.set(true);
        automationResizeLongestSide.set(1280);
        automationConvertEnabled.set(false);
        automationConvertFormat.set('jpg');
        automationCompressEnabled.set(true);
        automationCompressQuality.set(72);
        automationImageQuality.set(82);
        automationStripMetadataEnabled.set(false);
    } else if (recipeId === 'safe-share-images') {
        automationRecipeName.set('Safe Share Images');
        automationResizeEnabled.set(true);
        automationResizeLongestSide.set(1600);
        automationConvertEnabled.set(true);
        automationConvertFormat.set('jpg');
        automationCompressEnabled.set(true);
        automationCompressQuality.set(86);
        automationImageQuality.set(90);
        automationStripMetadataEnabled.set(true);
    } else {
        automationRecipeName.set('Custom image recipe');
    }
}

export const enabledAutomationSteps = derived(
    [automationResizeEnabled, automationConvertEnabled, automationCompressEnabled, automationStripMetadataEnabled],
    ([$resize, $convert, $compress, $strip]) => {
        const steps: AutomationStepId[] = [];
        if ($strip) steps.push('strip_metadata');
        if ($resize) steps.push('resize');
        if ($convert) steps.push('convert');
        if ($compress) steps.push('compress');
        return steps;
    },
);

export const selectedRecipeCard = derived(selectedAutomationRecipe, ($id) => builtInAutomationRecipes.find((recipe) => recipe.id === $id) ?? builtInAutomationRecipes[1]);

export const automationPlainSummary = derived(
    [automationFiles, automationOutputDir, automationResizeEnabled, automationResizeLongestSide, automationConvertEnabled, automationConvertFormat, automationCompressEnabled, automationCompressQuality, automationImageQuality, automationStripMetadataEnabled],
    ([$files, $outputDir, $resize, $longest, $convert, $format, $compress, $compressQuality, $imageQuality, $strip]) => {
        const will: string[] = [];
        const wont: string[] = [];
        will.push(`Use ${$files.length} selected image${$files.length === 1 ? '' : 's'}.`);
        will.push($outputDir ? `Save final files to: ${$outputDir}` : 'Save final files beside the original images.');
        if ($strip) will.push('Remove hidden metadata where the selected output format supports it.');
        if ($resize) will.push(`Resize copies so the longest side is at most ${$longest}px.`);
        if ($convert) will.push(`Create final files as ${$format.toUpperCase()} at quality ${$imageQuality}.`);
        if ($compress) will.push(`Compress final files at quality ${$compressQuality}.`);
        wont.push('KeepItLocal will not upload files.');
        wont.push('KeepItLocal will not change original files.');
        wont.push('KeepItLocal will not delete anything.');
        wont.push('KeepItLocal will not create intermediate files for every step.');
        return { will, wont };
    },
);

export const automationStats = derived([automationFinalResults, automationProgressByKey], ([$results, $progress]) => {
    const success = $results.filter((r) => r.success).length;
    const failed = $results.filter((r) => !r.success && r.error !== 'Cancelled').length;
    const cancelled = $results.filter((r) => r.error === 'Cancelled').length;
    const active = Object.values($progress).filter((p) => {
        const stage = p.stage.toLowerCase();
        return p.progress < 100 && stage !== 'done' && stage !== 'failed' && stage !== 'cancelled';
    }).length;
    return { success, failed, cancelled, active };
});

function successfulOutputs(results: ImageResult[]) {
    return results.filter((r) => r.success && r.output_path).map((r) => r.output_path);
}

function failedCount(results: ImageResult[]) {
    return results.filter((r) => !r.success && r.error !== 'Cancelled').length;
}

function cancelledIn(results: ImageResult[]) {
    return results.some((r) => r.error === 'Cancelled');
}

async function addActivity(item: Omit<AutomationActivityItem, 'id'>) {
    const id = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}_${Math.random().toString(16).slice(2)}`;
    const fullItem: AutomationActivityItem = { id, ...item };

    automationActivity.update((items) => [fullItem, ...items].slice(0, 80));

    try {
        const saved = await invoke<AutomationActivityItem[]>('add_automation_activity', { item: fullItem });
        automationActivity.set(saved);
        automationPersistenceError.set(null);
    } catch (error) {
        const message = String(error);
        automationPersistenceError.set(message);
        toast(`Could not save automation activity: ${message}`, 'error');
    }
}

export async function runImageAutomationRecipe() {
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

    if (get(automationProcessing)) return;

    const operationId = newOperationId('automation-image');
    const started = Date.now();
    automationOperationId.set(operationId);
    automationRunStartedAt.set(started);
    automationProcessing.set(true);
    automationCancelling.set(false);
    automationCurrentStep.set(null);
    automationFinalResults.set([]);
    automationProgressByKey.set({});
    automationLastError.set(null);
    await setBusy(true);
    const stopBusy = reportBusy('automation-recipes', `Running ${get(automationRecipeName).trim() || 'automation'}`);

    try {
        const results = await invoke<ImageResult[]>('run_image_automation', {
            options: {
                operation_id: operationId,
                paths: files,
                output_dir: get(automationOutputDir),
                suffix: '_automated',
                resize_enabled: get(automationResizeEnabled),
                resize_longest_side: get(automationResizeLongestSide),
                convert_enabled: get(automationConvertEnabled),
                output_format: get(automationConvertFormat),
                compress_enabled: get(automationCompressEnabled),
                compress_quality: get(automationCompressQuality),
                image_quality: get(automationImageQuality),
                strip_metadata: get(automationStripMetadataEnabled),
            },
        });

        automationFinalResults.set(results);
        const success = successfulOutputs(results).length;
        const failed = failedCount(results);
        const cancelled = get(automationCancelling) || cancelledIn(results);
        const finished = Date.now();
        const recipeName = get(automationRecipeName).trim() || 'Automation recipe';

        if (cancelled) {
            await addActivity({ recipe_name: recipeName, level: 'warning', started_at: started, finished_at: finished, input_count: files.length, success_count: success, failed_count: failed, cancelled: true, output_dir: get(automationOutputDir), message: `${success} output${success === 1 ? '' : 's'} created before cancellation.` });
            reportOperationOutcome({
                toolId: 'automation-recipes',
                kind: 'cancelled',
                notifyTitle: 'Automation cancelled',
                notifyMessage: `${success} output${success === 1 ? '' : 's'} created before cancellation`,
                toastMessage: 'Automation cancelled',
                activitySummary: `Automation "${recipeName}" cancelled`,
                activityDetails: `${success} output${success === 1 ? '' : 's'} created before cancel · ${files.length} input${files.length === 1 ? '' : 's'}`,
            });
        } else if (success > 0 && failed === 0) {
            await addActivity({ recipe_name: recipeName, level: 'success', started_at: started, finished_at: finished, input_count: files.length, success_count: success, failed_count: 0, cancelled: false, output_dir: get(automationOutputDir), message: `Created ${success} final output file${success === 1 ? '' : 's'}.` });
            reportOperationOutcome({
                toolId: 'automation-recipes',
                kind: 'success',
                notifyTitle: 'Automation finished',
                notifyMessage: `${success} output file${success === 1 ? '' : 's'} created`,
                toastMessage: `Automation finished: ${success} output${success === 1 ? '' : 's'}`,
                activitySummary: `Ran "${recipeName}" · created ${success} output${success === 1 ? '' : 's'}`,
            });
        } else if (success > 0) {
            await addActivity({ recipe_name: recipeName, level: 'warning', started_at: started, finished_at: finished, input_count: files.length, success_count: success, failed_count: failed, cancelled: false, output_dir: get(automationOutputDir), message: `${success} output${success === 1 ? '' : 's'} created, ${failed} failed.` });
            reportOperationOutcome({
                toolId: 'automation-recipes',
                kind: 'partial',
                notifyTitle: 'Automation finished with warnings',
                notifyMessage: `${success} output${success === 1 ? '' : 's'}, ${failed} failed`,
                toastMessage: `${success} done, ${failed} failed`,
                activitySummary: `Ran "${recipeName}" with errors`,
                activityDetails: `${success} ok · ${failed} failed`,
            });
        } else {
            await addActivity({ recipe_name: recipeName, level: 'error', started_at: started, finished_at: finished, input_count: files.length, success_count: 0, failed_count: failed || files.length, cancelled: false, output_dir: get(automationOutputDir), message: 'No output files were created.' });
            reportOperationOutcome({
                toolId: 'automation-recipes',
                kind: 'failed',
                notifyTitle: 'Automation failed',
                notifyMessage: 'No output files were created',
                toastMessage: 'Automation failed',
                activitySummary: `Automation "${recipeName}" failed`,
                activityDetails: `No output files created · ${failed || files.length} failed`,
            });
        }
    } catch (error) {
        const message = String(error);
        const cancelled = get(automationCancelling);
        automationLastError.set(message);
        await addActivity({ recipe_name: get(automationRecipeName).trim() || 'Automation recipe', level: cancelled ? 'warning' : 'error', started_at: started, finished_at: Date.now(), input_count: files.length, success_count: 0, failed_count: cancelled ? 0 : files.length, cancelled, output_dir: get(automationOutputDir), message });
        reportOperationOutcome({
            toolId: 'automation-recipes',
            kind: cancelled ? 'cancelled' : 'failed',
            notifyTitle: cancelled ? 'Automation cancelled' : 'Automation failed',
            notifyMessage: message,
            activitySummary: cancelled ? `Automation cancelled` : `Automation failed`,
            activityDetails: message.slice(0, 200),
        });
    } finally {
        automationProcessing.set(false);
        automationCancelling.set(false);
        automationCurrentStep.set(null);
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelImageAutomationRecipe() {
    if (!get(automationProcessing) || get(automationCancelling)) return;
    automationCancelling.set(true);
    await cancelImageOperation(get(automationOperationId));
}

export async function saveCurrentAutomationRecipe() {
    const name = get(automationRecipeName).trim() || 'Untitled automation';
    const recipe: SavedAutomationRecipe = {
        id: globalThis.crypto?.randomUUID?.() ?? `${Date.now()}_${Math.random().toString(16).slice(2)}`,
        name,
        recipe_id: get(selectedAutomationRecipe),
        resize_enabled: get(automationResizeEnabled),
        resize_longest_side: get(automationResizeLongestSide),
        convert_enabled: get(automationConvertEnabled),
        convert_format: get(automationConvertFormat),
        compress_enabled: get(automationCompressEnabled),
        compress_quality: get(automationCompressQuality),
        image_quality: get(automationImageQuality),
        strip_metadata_enabled: get(automationStripMetadataEnabled),
        created_at: Date.now(),
    };

    const next = [recipe, ...get(savedAutomationRecipes).filter((item) => item.name !== name)].slice(0, 40);
    savedAutomationRecipes.set(next);
    await persistSavedRecipes(next);
    toast('Recipe saved', 'success');
}

export function loadAutomationRecipe(recipe: SavedAutomationRecipe) {
    if (get(automationProcessing)) return;
    selectedAutomationRecipe.set(recipe.recipe_id);
    automationRecipeName.set(recipe.name);
    automationResizeEnabled.set(recipe.resize_enabled);
    automationResizeLongestSide.set(recipe.resize_longest_side);
    automationConvertEnabled.set(recipe.convert_enabled);
    automationConvertFormat.set(recipe.convert_format);
    automationCompressEnabled.set(recipe.compress_enabled);
    automationCompressQuality.set(recipe.compress_quality);
    automationImageQuality.set(recipe.image_quality);
    automationStripMetadataEnabled.set(recipe.strip_metadata_enabled);
    automationView.set('setup');
    toast(`Loaded ${recipe.name}`, 'info');
}

export async function deleteAutomationRecipe(id: string) {
    const next = get(savedAutomationRecipes).filter((item) => item.id !== id);
    savedAutomationRecipes.set(next);
    await persistSavedRecipes(next);
}

export async function clearAutomationActivity() {
    automationActivity.set([]);
    try {
        await invoke('clear_automation_activity');
        automationPersistenceError.set(null);
        toast('Automation activity cleared', 'success');
    } catch (error) {
        const message = String(error);
        automationPersistenceError.set(message);
        toast(`Could not clear automation activity: ${message}`, 'error');
    }
}

export async function openAutomationDataFolder() {
    try {
        const path = await invoke<string>('open_automation_data_folder');
        toast(`Automation data folder: ${path}`, 'info');
    } catch (error) {
        toast(String(error), 'error');
    }
}

export function readableStepName(step: AutomationStepId | null) {
    if (step === 'strip_metadata') return 'Cleaning metadata';
    if (step === 'resize') return 'Resizing images';
    if (step === 'convert') return 'Converting images';
    if (step === 'compress') return 'Compressing images';
    return 'Preparing automation';
}

export function recipeActionSummary() {
    const parts: string[] = [];
    if (get(automationStripMetadataEnabled)) parts.push('clean metadata');
    if (get(automationResizeEnabled)) parts.push(`resize ${get(automationResizeLongestSide)}px`);
    if (get(automationConvertEnabled)) parts.push(`convert ${get(automationConvertFormat).toUpperCase()}`);
    if (get(automationCompressEnabled)) parts.push(`compress ${get(automationCompressQuality)}`);
    return parts.length ? parts.join(' → ') : 'No actions selected';
}

export { fileName, fmtBytes };
