/*
  myCommands — user-defined commands, quicklinks, and custom bangs for
  the command palette.

  ONE unified model (per the FeaturesIdeas decision): a quicklink is a
  command whose action opens a URL; a custom bang is a command with a
  {query} placeholder; an "open chrome" command launches an app. All
  three share this shape, so the palette + Settings manage one list.

  Persistence: localStorage (`keepitlocal_my_commands_v1`). This SURVIVES
  RESTART — Tauri's webview persists localStorage in the app data dir, the
  same mechanism tool packs / onboarding / command-appearance already use.

  Cross-window sync: the command palette runs in its own Tauri window; the
  management UI lives in the main window's Settings. Both read/write the
  same key. The `storage` event fires in OTHER windows when one writes, so
  each window reloads to stay in sync live (mirrors commandAppearance.ts).
  The `applyingExternal` guard stops the reload from re-writing (which
  would ping-pong the storage event between windows).
*/

import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

export type MyCommandType = 'url' | 'bang' | 'app' | 'file' | 'folder' | 'shell';

/**
 * Which shell to invoke for `type === 'shell'` commands. Wave 4.1b-2
 * (2026-05-27).
 *   - 'powershell' (default) — Windows PowerShell 5.1 via powershell.exe.
 *     Ships with every modern Windows; covers ~all user shell needs
 *     (git, npm, dir, ls, cargo, curl, etc.).
 *   - 'pwsh'                 — PowerShell 7+ via pwsh.exe. Only works
 *     when the user has installed Core. Same flag set as powershell.
 *   - 'cmd'                  — cmd.exe, the legacy command interpreter,
 *     for `for /f` / native `.bat` quirks and Windows-y "what would
 *     I type at a cmd prompt" reflexes.
 * Only applies when type === 'shell'; ignored otherwise.
 */
export type MyShellKind = 'powershell' | 'pwsh' | 'cmd';

const VALID_SHELL_KINDS: MyShellKind[] = ['powershell', 'pwsh', 'cmd'];

export interface MyCommand {
    /** Stable unique id (timestamp + random suffix). */
    id: string;
    /** Trigger keyword, lowercased, no spaces. */
    keyword: string;
    /** Human label shown in the palette + Settings. */
    label: string;
    /** What kind of target this opens. */
    type: MyCommandType;
    /**
     * The target:
     *   url    → a full URL ("https://github.com/me/repo")
     *   bang   → a URL template with {query} ("https://github.com/search?q={query}")
     *   app    → an executable / app path
     *   file   → a file path
     *   folder → a folder path
     *   shell  → a shell command string (PowerShell `-Command` payload). Wave 4.1
     *            (2026-05-27). Trust model is the same as installing any
     *            Windows app — the user owns the command, they own the
     *            consequence. The first time a shell command runs in a
     *            session, the palette shows a one-time warning toast.
     */
    target: string;
    /** Optional free-text description shown in Settings + the palette's
     *  action panel hint. Wave 4.1 — helps the user remember what each
     *  custom command does, especially useful for shell commands. */
    description?: string;
    /** Which shell to run when type === 'shell'. Defaults to
     *  'powershell' when omitted (matches Wave 4.1 behavior so existing
     *  saved commands keep working). Ignored for non-shell types. */
    shellKind?: MyShellKind;
}

const STORAGE_KEY = 'keepitlocal_my_commands_v1';

const VALID_TYPES: MyCommandType[] = ['url', 'bang', 'app', 'file', 'folder', 'shell'];

function isValidCommand(c: unknown): c is MyCommand {
    if (!c || typeof c !== 'object') return false;
    const r = c as Record<string, unknown>;
    return (
        typeof r.id === 'string' &&
        typeof r.keyword === 'string' &&
        typeof r.label === 'string' &&
        typeof r.target === 'string' &&
        typeof r.type === 'string' &&
        VALID_TYPES.includes(r.type as MyCommandType) &&
        (r.description === undefined || typeof r.description === 'string') &&
        (r.shellKind === undefined ||
            (typeof r.shellKind === 'string' &&
                VALID_SHELL_KINDS.includes(r.shellKind as MyShellKind)))
    );
}

/** Parse a stored JSON array of commands, defensively. */
function parseCommands(raw: string | null | undefined): MyCommand[] {
    if (!raw) return [];
    try {
        const parsed = JSON.parse(raw);
        return Array.isArray(parsed) ? parsed.filter(isValidCommand) : [];
    } catch {
        return [];
    }
}

/** Read the commands from the DPAPI-encrypted backend store. */
async function loadFromBackend(): Promise<MyCommand[]> {
    try {
        const raw = await invoke<string | null>('secure_kv_get', { key: STORAGE_KEY });
        return parseCommands(raw);
    } catch {
        return [];
    }
}

/** Persist the commands to the encrypted backend. Best-effort. */
async function save(cmds: MyCommand[]): Promise<void> {
    try {
        await invoke('secure_kv_set', { key: STORAGE_KEY, value: JSON.stringify(cmds) });
    } catch {
        // Backend unavailable — best effort; the in-memory store stays usable.
    }
}

/** One-time migration: if the encrypted backend is empty but the old plaintext
 *  localStorage value exists, move it into the backend and DROP the plaintext
 *  copy — sensitive commands (shell payloads, paths) shouldn't sit in plaintext.
 *  Returns the migrated commands, or null when there's nothing to migrate. */
async function migrateFromLocalStorage(): Promise<MyCommand[] | null> {
    if (typeof localStorage === 'undefined') return null;
    const legacy = localStorage.getItem(STORAGE_KEY);
    if (!legacy) return null;
    const cmds = parseCommands(legacy);
    try {
        await invoke('secure_kv_set', { key: STORAGE_KEY, value: JSON.stringify(cmds) });
        localStorage.removeItem(STORAGE_KEY); // drop the plaintext copy
    } catch {
        // Write failed — keep the legacy value so we retry next launch rather
        // than lose the user's commands.
        return null;
    }
    return cmds;
}

export const myCommands = writable<MyCommand[]>([]);

/** Suppresses the auto-save while we apply a backend load/reload, AND until the
 *  first load completes — otherwise the initial empty store would clobber the
 *  persisted commands. */
let applyingExternal = true;

myCommands.subscribe((cmds) => {
    if (applyingExternal) return;
    void save(cmds);
});

/** Load commands from the encrypted backend (migrating any legacy plaintext
 *  localStorage value on first run), then seed the built-in defaults. Runs on
 *  import in every window; the backend redb is the shared source of truth. */
export async function initMyCommands(): Promise<void> {
    let cmds = await loadFromBackend();
    if (cmds.length === 0) {
        const migrated = await migrateFromLocalStorage();
        if (migrated) cmds = migrated;
    }
    applyingExternal = true;
    myCommands.set(cmds);
    applyingExternal = false; // saves are now live
    seedDefaults(); // may append new built-in bangs → triggers a backend save
}

/**
 * Force a re-read from the encrypted backend.
 *
 * The command palette runs in its own Tauri window that's created once and
 * reused, so a command added in the main window's My Commands tool won't be in
 * the palette's in-memory store. The palette calls this on every summon to pick
 * up changes. The backend redb is shared across the app's windows, so the
 * reload is authoritative. `applyingExternal` is raised around the set so the
 * reloaded value isn't immediately re-saved.
 */
export async function reloadMyCommands(): Promise<void> {
    const cmds = await loadFromBackend();
    applyingExternal = true;
    myCommands.set(cmds);
    applyingExternal = false;
}

/* ──────────────────────────────────────────────────────────────────────
   Built-in default commands (the "bangs" that ship with KeepItLocal).

   These are SEEDED into the SAME store + localStorage key custom commands
   live in — so they show in the unified My Commands list, are matched by
   the palette's My Commands matcher (working even when the backend Web-
   search opt-in is OFF, because an explicitly-typed bang is a user-
   initiated action), and are fully editable / deletable like any command.

   Mirrors src-tauri/src/commands/quick_actions.rs (BANG_DEFS) — keep the
   two in sync when adding a provider. Each carries a stable `builtin-*`
   id so the one-time seeder can add NEW defaults in a future update
   without duplicating existing ones or resurrecting ones the user deleted.
   ────────────────────────────────────────────────────────────────────── */
export const DEFAULT_COMMANDS: MyCommand[] = [
    { id: 'builtin-google', keyword: 'g', label: 'Google', type: 'bang', target: 'https://www.google.com/search?q={query}' },
    { id: 'builtin-ddg', keyword: 'ddg', label: 'DuckDuckGo', type: 'bang', target: 'https://duckduckgo.com/?q={query}' },
    { id: 'builtin-bing', keyword: 'bing', label: 'Bing', type: 'bang', target: 'https://www.bing.com/search?q={query}' },
    { id: 'builtin-brave', keyword: 'brave', label: 'Brave Search', type: 'bang', target: 'https://search.brave.com/search?q={query}' },
    { id: 'builtin-kagi', keyword: 'kagi', label: 'Kagi', type: 'bang', target: 'https://kagi.com/search?q={query}' },
    { id: 'builtin-startpage', keyword: 'sp', label: 'Startpage', type: 'bang', target: 'https://www.startpage.com/sp/search?q={query}' },
    { id: 'builtin-wikipedia', keyword: 'w', label: 'Wikipedia', type: 'bang', target: 'https://en.wikipedia.org/w/index.php?search={query}' },
    { id: 'builtin-translate', keyword: 'tr', label: 'Google Translate', type: 'bang', target: 'https://translate.google.com/?text={query}' },
    { id: 'builtin-maps', keyword: 'maps', label: 'Google Maps', type: 'bang', target: 'https://www.google.com/maps/search/{query}' },
    { id: 'builtin-images', keyword: 'img', label: 'Google Images', type: 'bang', target: 'https://www.google.com/search?tbm=isch&q={query}' },
    { id: 'builtin-github', keyword: 'gh', label: 'GitHub', type: 'bang', target: 'https://github.com/search?q={query}' },
    { id: 'builtin-stackoverflow', keyword: 'so', label: 'Stack Overflow', type: 'bang', target: 'https://stackoverflow.com/search?q={query}' },
    { id: 'builtin-mdn', keyword: 'mdn', label: 'MDN', type: 'bang', target: 'https://developer.mozilla.org/en-US/search?q={query}' },
    { id: 'builtin-crates', keyword: 'crates', label: 'crates.io', type: 'bang', target: 'https://crates.io/search?q={query}' },
    { id: 'builtin-docsrs', keyword: 'docs', label: 'docs.rs', type: 'bang', target: 'https://docs.rs/?q={query}' },
    { id: 'builtin-npm', keyword: 'npm', label: 'npm', type: 'bang', target: 'https://www.npmjs.com/search?q={query}' },
    { id: 'builtin-pypi', keyword: 'pypi', label: 'PyPI', type: 'bang', target: 'https://pypi.org/search/?q={query}' },
    { id: 'builtin-youtube', keyword: 'yt', label: 'YouTube', type: 'bang', target: 'https://www.youtube.com/results?search_query={query}' },
    { id: 'builtin-amazon', keyword: 'amz', label: 'Amazon', type: 'bang', target: 'https://www.amazon.com/s?k={query}' },
    { id: 'builtin-reddit', keyword: 'r/', label: 'Reddit (subreddit)', type: 'bang', target: 'https://www.reddit.com/r/{query}' },
];

/** True for a command that was seeded from DEFAULT_COMMANDS (vs. authored
 *  by the user). Used by Settings to tag it; functionally it's identical
 *  to any other command. */
export function isBuiltinCommand(cmd: MyCommand): boolean {
    return cmd.id.startsWith('builtin-');
}

const SEEDED_KEY = 'keepitlocal_my_commands_seeded_v1';

function loadSeededIds(): Set<string> {
    if (typeof localStorage === 'undefined') return new Set();
    try {
        const raw = localStorage.getItem(SEEDED_KEY);
        if (!raw) return new Set();
        const parsed = JSON.parse(raw);
        return Array.isArray(parsed)
            ? new Set(parsed.filter((x): x is string => typeof x === 'string'))
            : new Set();
    } catch {
        return new Set();
    }
}

function saveSeededIds(ids: Set<string>) {
    if (typeof localStorage === 'undefined') return;
    try {
        localStorage.setItem(SEEDED_KEY, JSON.stringify([...ids]));
    } catch {
        // best effort
    }
}

/**
 * One-time, idempotent seed of the built-in defaults into the user's list.
 * Each default is offered at most once EVER (its id is recorded in
 * SEEDED_KEY), so: first run seeds the current set; a default the user
 * later deletes stays gone; a brand-new default shipped in an update is
 * seeded once on the next launch. A default whose keyword is already taken
 * by a user command is skipped (and still marked seeded) so we never
 * create a duplicate trigger. Runs at module load — localStorage is shared
 * across windows and the main window initializes first, so by the time the
 * palette window loads the set is already seeded (it just reads it).
 */
function seedDefaults() {
    const seeded = loadSeededIds();
    const current = get(myCommands);
    const haveKeyword = new Set(current.map((c) => c.keyword));
    const haveId = new Set(current.map((c) => c.id));
    const toAdd: MyCommand[] = [];
    let seededChanged = false;
    for (const def of DEFAULT_COMMANDS) {
        if (seeded.has(def.id)) continue; // already offered once — respect deletes
        seeded.add(def.id);
        seededChanged = true;
        if (haveId.has(def.id) || haveKeyword.has(def.keyword)) continue; // collision
        toAdd.push({ ...def });
        haveKeyword.add(def.keyword);
    }
    // Append so the user's own commands keep priority in the list + the
    // palette's top-N match slice.
    if (toAdd.length) myCommands.update((list) => [...list, ...toAdd]);
    if (seededChanged) saveSeededIds(seeded);
}

// Kick off the async load (backend → migrate any legacy plaintext localStorage
// → seed defaults) on import, in whichever window loaded this module.
void initMyCommands();

/** Normalize a keyword: lowercase, trim, collapse internal whitespace to
 *  nothing (keywords are single tokens). */
export function normalizeKeyword(raw: string): string {
    return raw.trim().toLowerCase().replace(/\s+/g, '');
}

function makeId(): string {
    return `cmd-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/** Ensure a keyword is unique among existing commands (excluding `exceptId`
 *  when editing) by appending -2, -3, … if taken. */
function uniqueKeyword(keyword: string, exceptId?: string): string {
    const existing = get(myCommands);
    const taken = new Set(
        existing.filter((c) => c.id !== exceptId).map((c) => c.keyword),
    );
    if (!taken.has(keyword)) return keyword;
    let n = 2;
    while (taken.has(`${keyword}-${n}`)) n++;
    return `${keyword}-${n}`;
}

/** Add a command. Returns the created record (with its assigned id +
 *  uniqued keyword). */
export function addCommand(input: Omit<MyCommand, 'id'>): MyCommand {
    const keyword = uniqueKeyword(normalizeKeyword(input.keyword) || 'cmd');
    const cmd: MyCommand = {
        id: makeId(),
        keyword,
        label: input.label.trim() || keyword,
        type: input.type,
        target: input.target.trim(),
        description: input.description?.trim() || undefined,
        // Only carry shellKind for shell-type commands; for everything
        // else we drop it so the persisted JSON stays clean.
        shellKind:
            input.type === 'shell'
                ? input.shellKind && VALID_SHELL_KINDS.includes(input.shellKind)
                    ? input.shellKind
                    : 'powershell'
                : undefined,
    };
    myCommands.update((list) => [...list, cmd]);
    return cmd;
}

/** Patch an existing command by id. Keyword is re-uniqued if changed. */
export function updateCommand(id: string, patch: Partial<Omit<MyCommand, 'id'>>) {
    myCommands.update((list) =>
        list.map((c) => {
            if (c.id !== id) return c;
            const next = { ...c, ...patch };
            if (patch.keyword !== undefined) {
                next.keyword = uniqueKeyword(normalizeKeyword(patch.keyword) || 'cmd', id);
            }
            next.label = (next.label ?? '').trim() || next.keyword;
            next.target = (next.target ?? '').trim();
            // Normalize description: empty string → undefined so JSON
            // doesn't carry empty noise around.
            if ('description' in patch) {
                next.description = (patch.description ?? '').trim() || undefined;
            }
            // shellKind only valid for shell-type commands. When the
            // type changes AWAY from shell, drop it; when it changes
            // INTO shell with no kind set, default to powershell.
            if (next.type === 'shell') {
                if (!next.shellKind || !VALID_SHELL_KINDS.includes(next.shellKind)) {
                    next.shellKind = 'powershell';
                }
            } else {
                next.shellKind = undefined;
            }
            return next;
        }),
    );
}

export function removeCommand(id: string) {
    myCommands.update((list) => list.filter((c) => c.id !== id));
}

/** True when a command of type 'bang' (has a {query} placeholder). */
export function isBang(cmd: MyCommand): boolean {
    return cmd.type === 'bang' || cmd.target.includes('{query}');
}

/** Substitute the user's query into a bang target. */
export function expandBang(cmd: MyCommand, query: string): string {
    return cmd.target.replace(/\{query\}/g, encodeURIComponent(query.trim()));
}
