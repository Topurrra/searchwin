import { writable, derived } from 'svelte/store';

export type NotificationLevel = 'info' | 'success' | 'warning' | 'error';

export interface AppNotification {
    id: string;
    level: NotificationLevel;
    title: string;
    message?: string;
    timestamp: number;
    read: boolean;
    toolId?: string;
}

const STORAGE_KEY = 'keepitlocal_notifications_v1';
const MAX_NOTIFICATIONS = 10;

function load(): AppNotification[] {
    if (typeof localStorage === 'undefined') return [];
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        return raw ? JSON.parse(raw) : [];
    } catch {
        return [];
    }
}

function save(items: AppNotification[]) {
    if (typeof localStorage === 'undefined') return;
    localStorage.setItem(STORAGE_KEY, JSON.stringify(items));
}

export const notifications = writable<AppNotification[]>(load());

notifications.subscribe(save);

export const unreadCount = derived(notifications, $n => $n.filter(x => !x.read).length);

export function notify(item: Omit<AppNotification, 'id' | 'timestamp' | 'read'>) {
    const n: AppNotification = {
        ...item,
        id: `${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
        timestamp: Date.now(),
        read: false,
    };
    notifications.update(arr => [n, ...arr].slice(0, MAX_NOTIFICATIONS));
    return n.id;
}

export function markRead(id: string) {
    notifications.update(arr => arr.map(n => (n.id === id ? { ...n, read: true } : n)));
}

export function markAllRead() {
    notifications.update(arr => arr.map(n => ({ ...n, read: true })));
}

export function clearNotifications() {
    notifications.set([]);
}