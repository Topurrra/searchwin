// Time Tracker — typed frontend bindings over the Rust commands, plus local-time
// date helpers (the backend stores per-local-day blobs and aggregates exactly
// the date list we pass, so all date math happens here where the locale is known).

import { invoke } from '@tauri-apps/api/core';
import { writable } from 'svelte/store';

export type CategoryKind = 'productive' | 'neutral' | 'distracting';
export type TimeTrackerTab = 'dashboard' | 'settings';
export type TimeTrackerView = 'day' | 'week';

export interface CategoryDef {
    name: string;
    color: string;
    kind: CategoryKind;
}

export interface CategoryRule {
    field: 'app' | 'title';
    op: 'is' | 'contains';
    pattern: string;
    category: string;
}

export interface Goal {
    category: string;
    dailyTargetMins: number;
}

export interface TrackerConfig {
    enabled: boolean;
    captureTitles: boolean;
    idleThresholdSecs: number;
    retentionDays: number;
    pausedUntilMs: number;
    excludedApps: string[];
    categories: CategoryDef[];
    rules: CategoryRule[];
    goals: Goal[];
}

export interface TrackerStatus {
    enabled: boolean;
    paused: boolean;
    pausedUntilMs: number;
    tracking: boolean;
    currentApp: string;
    currentCategory: string;
}

export interface ActivityEvent {
    startMs: number;
    endMs: number;
    app: string;
    title: string;
    category: string;
    idle: boolean;
}

export interface CategorySummary {
    name: string;
    color: string;
    kind: CategoryKind;
    secs: number;
}

export interface AppSummary {
    app: string;
    category: string;
    secs: number;
}

export interface DaySummary {
    date: string;
    activeSecs: number;
    idleSecs: number;
}

export interface GoalProgress {
    category: string;
    targetMins: number;
    actualMins: number;
}

export interface RangeSummary {
    totalActiveSecs: number;
    totalIdleSecs: number;
    focusScore: number;
    byCategory: CategorySummary[];
    byApp: AppSummary[];
    byDay: DaySummary[];
    timeline: ActivityEvent[];
    goals: GoalProgress[];
    categories: CategoryDef[];
}

export const getConfig = () => invoke<TrackerConfig>('time_tracker_get_config');
export const setConfig = (config: TrackerConfig) => invoke<void>('time_tracker_set_config', { config });
export const pauseTracking = (minutes: number) => invoke<void>('time_tracker_pause', { minutes });
export const getStatus = () => invoke<TrackerStatus>('time_tracker_status');
export const getRange = (dates: string[]) => invoke<RangeSummary>('time_tracker_range', { dates });
export const wipeData = () => invoke<void>('time_tracker_wipe');

// ── Local-time date helpers ─────────────────────────────────────────────────

const pad = (n: number) => String(n).padStart(2, '0');

/** A Date → local `YYYY-MM-DD` (matches the backend's GetLocalTime day keys). */
export function localDate(d: Date): string {
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function todayKey(): string {
    return localDate(new Date());
}

export const timeTrackerTab = writable<TimeTrackerTab>('dashboard');
export const timeTrackerView = writable<TimeTrackerView>('day');
export const timeTrackerRefDate = writable(todayKey());

export function addDays(date: string, delta: number): string {
    const d = parseDate(date);
    d.setDate(d.getDate() + delta);
    return localDate(d);
}

export function parseDate(date: string): Date {
    const [y, m, d] = date.split('-').map(Number);
    return new Date(y, (m ?? 1) - 1, d ?? 1);
}

/** Monday-anchored week containing `date` → seven `YYYY-MM-DD` strings. */
export function weekDates(date: string): string[] {
    const d = parseDate(date);
    const dow = (d.getDay() + 6) % 7; // 0 = Monday
    d.setDate(d.getDate() - dow);
    return Array.from({ length: 7 }, (_, i) => {
        const day = new Date(d);
        day.setDate(d.getDate() + i);
        return localDate(day);
    });
}

/** Compact, humane duration: "2h 14m", "47m", "38s", or "0m". */
export function formatDuration(secs: number): string {
    if (!secs || secs < 0) return '0m';
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    if (h > 0) return m > 0 ? `${h}h ${m}m` : `${h}h`;
    if (m > 0) return `${m}m`;
    return `${secs}s`;
}

const WEEKDAY = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
export function weekdayLabel(date: string): string {
    const d = parseDate(date);
    return WEEKDAY[(d.getDay() + 6) % 7];
}

export function prettyDate(date: string): string {
    const d = parseDate(date);
    return d.toLocaleDateString(undefined, { weekday: 'long', month: 'short', day: 'numeric' });
}
