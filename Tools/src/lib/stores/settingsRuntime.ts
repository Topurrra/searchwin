import { writable } from 'svelte/store';

export type SettingsInstalledApp = { name: string; path: string };

export type SettingsStorageInsights = {
    preferencesDbBytes: number;
    clipboardImagesBytes: number;
    clipboardImagesCount: number;
    fileSearchIndexBytes: number;
    automationBytes: number;
    totalBytes: number;
    quarantineCount: number;
    quarantineBytes: number;
};

export type SettingsAppStoragePaths = {
    appDataDir: string;
    preferencesDatabase: string;
    automationDir: string;
    automationDatabase: string;
    automationActivityDb: string;
    fileSearchIndexDir: string;
    fileSearchDatabase: string;
    clipboardImagesDir: string;
    clipboardHistoryJson: string;
};

// Session-only state for longer Settings actions. Sensitive backup payloads and
// chooser paths deliberately remain inside the action that is currently running.
export const settingsInstalledApps = writable<SettingsInstalledApp[]>([]);
export const settingsInstalledAppsLoading = writable(false);
export const settingsStorageInsights = writable<SettingsStorageInsights | null>(null);
export const settingsStoragePaths = writable<SettingsAppStoragePaths | null>(null);
export const settingsStorageBusy = writable(false);
export const settingsExportBusy = writable(false);
export const settingsImportBusy = writable(false);
