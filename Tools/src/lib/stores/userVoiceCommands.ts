/*
  userVoiceCommands — user-authored voice commands (#21).

  Voice upgrade, #21. Power users define their own spoken commands in a
  plain-text file (the KeepItLocal equivalent of a Talon command file).
  This module is the FRONTEND half: it reads that file via the Rust
  `voice_scripts` commands, PARSES it into `VoiceCommandDefinition`s, and
  hot-reloads when the file changes on disk.

  The format — one command per line, `phrase = action`:
    save my work = key ctrl+s
    check my email = url https://mail.google.com
  Section headers scope the commands below them to one app's window:
    [app: chrome]      …only while Chrome is focused
    [app: *]           …everywhere again (the default)
  Lines starting with `#` are comments; blank lines are ignored.

  Every parsed command is a `literal`, Medium-risk command: its phrase
  is the COMPLETE spoken phrase (no verb stripped), matched against the
  whole transcript and emitted into the Vosk grammar verbatim — exactly
  like the #18a keyboard / mouse commands. `commandRegistry` folds
  `getUserVoiceCommands()` into `buildVoiceCommandRegistry`, so user
  commands reach both the matcher and the grammar with no drift.

  Module boundary: this is a LEAF. It type-imports `VoiceCommandDefinition`
  / `VoiceAction` (erased at build) and runtime-imports only Tauri APIs
  and `svelte/store` — never `commandRegistry`, so the registry can
  import THIS module with no runtime cycle.
*/

import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { VoiceCommandDefinition } from './commandRegistry';
import type { VoiceAction } from './voiceActionExecutor';

/** A line the parser could not turn into a command — surfaced in
 *  Settings so the user can fix it. */
export interface UserCommandParseError {
    /** 1-based line number in the command file. */
    line: number;
    /** The offending line, trimmed. */
    text: string;
    /** Why it was rejected. */
    message: string;
}

/** The parsed state of the user command file. */
export interface UserVoiceCommandsState {
    /** Ready-to-register command definitions, app-scoped ones first so
     *  a scoped command out-ranks a same-phrase global one. */
    commands: VoiceCommandDefinition[];
    /** Per-line parse errors. */
    errors: UserCommandParseError[];
    /** True once the file has been read at least once this session. */
    loaded: boolean;
}

const EMPTY: UserVoiceCommandsState = { commands: [], errors: [], loaded: false };

const store = writable<UserVoiceCommandsState>(EMPTY);

/** Read-only store handle — drives the Settings → Voice panel. */
export const userVoiceCommands = { subscribe: store.subscribe };

/** Synchronous snapshot — `buildVoiceCommandRegistry` (sync) reads this;
 *  the store drives UI. Kept in lock-step with every `store.set`. */
let snapshot: UserVoiceCommandsState = EMPTY;

// ─── Parser ──────────────────────────────────────────────────────────

/** Named keys the keystroke backend (`voice_input.rs::key_to_vk`)
 *  understands — kept in sync so the parser can reject typos early. */
const NAMED_KEYS = new Set([
    'enter', 'return', 'escape', 'esc', 'tab', 'space', 'spacebar',
    'backspace', 'delete', 'del', 'up', 'down', 'left', 'right', 'home',
    'end', 'page up', 'pageup', 'page down', 'pagedown',
    'f1', 'f2', 'f3', 'f4', 'f5', 'f6', 'f7', 'f8', 'f9', 'f10', 'f11', 'f12',
]);

/** Modifier names the backend (`modifier_to_vk`) accepts. */
const MODIFIERS = new Set([
    'ctrl', 'control', 'alt', 'shift', 'win', 'windows', 'super', 'meta',
]);

/** A key is valid if it is a named key or a single alphanumeric char. */
function isValidKey(key: string): boolean {
    if (NAMED_KEYS.has(key)) return true;
    return key.length === 1 && /[a-z0-9]/i.test(key);
}

/** Normalize a spoken phrase: lowercase, drop digits / punctuation
 *  (Unicode-aware so Georgian survives), collapse whitespace. The Vosk
 *  grammar takes plain words — this is what the matcher compares. */
function normalizePhrase(text: string): string {
    return text
        .toLowerCase()
        .replace(/[^\p{L}\s]+/gu, ' ')
        .replace(/\s+/g, ' ')
        .trim();
}

/** Parse the right-hand side of a command line into a `VoiceAction`. */
function parseAction(spec: string): { action: VoiceAction } | { error: string } {
    const space = spec.indexOf(' ');
    const kind = (space < 0 ? spec : spec.slice(0, space)).toLowerCase();
    const arg = space < 0 ? '' : spec.slice(space + 1).trim();

    if (kind === 'key') {
        if (arg.length === 0) {
            return { error: 'a "key" action needs a shortcut, e.g. key ctrl+s' };
        }
        const parts = arg
            .split('+')
            .map((p) => p.trim().toLowerCase())
            .filter((p) => p.length > 0);
        if (parts.length === 0) {
            return { error: `could not read the shortcut "${arg}"` };
        }
        const key = parts[parts.length - 1];
        const modifiers = parts.slice(0, -1);
        for (const modifier of modifiers) {
            if (!MODIFIERS.has(modifier)) {
                return {
                    error: `unknown modifier "${modifier}" — use ctrl, alt, shift, or win`,
                };
            }
        }
        if (!isValidKey(key)) {
            return { error: `unknown key "${key}"` };
        }
        return { action: { kind: 'keystroke', key, modifiers } };
    }

    if (kind === 'url') {
        if (arg.length === 0) {
            return { error: 'a "url" action needs a web address' };
        }
        const url = /^https?:\/\//i.test(arg) ? arg : `https://${arg}`;
        return { action: { kind: 'open-url', url } };
    }

    return { error: `unknown action "${kind}" — use "key" or "url"` };
}

/**
 * Parse the full command file into command definitions + parse errors.
 * Pure — no IO. Exported for the test suite (#26).
 */
export function parseUserVoiceCommands(text: string): {
    commands: VoiceCommandDefinition[];
    errors: UserCommandParseError[];
} {
    const commands: VoiceCommandDefinition[] = [];
    const errors: UserCommandParseError[] = [];
    /** Phrases already defined, keyed by `<scope>::<phrase>`, to flag
     *  duplicates (the matcher would silently shadow the later one). */
    const seen = new Set<string>();
    /** The active section's app scope; `undefined` ⇒ global. */
    let appScope: string | undefined;
    let id = 0;

    text.split(/\r?\n/).forEach((raw, i) => {
        const lineNo = i + 1;
        const line = raw.trim();
        if (line.length === 0 || line.startsWith('#')) return;

        // Section header — [app: name], [app: *], or [global].
        if (line.startsWith('[')) {
            const appHeader = line.match(/^\[\s*app\s*:\s*(.+?)\s*\]$/i);
            if (appHeader) {
                const name = appHeader[1].trim().toLowerCase();
                appScope =
                    name === '*' || name === 'any'
                        ? undefined
                        : name.replace(/\.exe$/, '');
                return;
            }
            if (/^\[\s*global\s*\]$/i.test(line)) {
                appScope = undefined;
                return;
            }
            errors.push({
                line: lineNo,
                text: line,
                message: 'not a valid section header — use [app: name] or [app: *]',
            });
            return;
        }

        // Command line — phrase = action (split on the FIRST "=" so a
        // "=" inside a URL query string survives).
        const eq = line.indexOf('=');
        if (eq < 0) {
            errors.push({
                line: lineNo,
                text: line,
                message: 'missing "=" — write the line as: phrase = action',
            });
            return;
        }
        const phrase = normalizePhrase(line.slice(0, eq));
        const spec = line.slice(eq + 1).trim();
        if (phrase.length === 0) {
            errors.push({
                line: lineNo,
                text: line,
                message: 'the phrase has no usable words',
            });
            return;
        }
        if (spec.length === 0) {
            errors.push({
                line: lineNo,
                text: line,
                message: 'missing the action after "="',
            });
            return;
        }
        const parsed = parseAction(spec);
        if ('error' in parsed) {
            errors.push({ line: lineNo, text: line, message: parsed.error });
            return;
        }
        const dedupeKey = `${appScope ?? '*'}::${phrase}`;
        if (seen.has(dedupeKey)) {
            errors.push({
                line: lineNo,
                text: line,
                message: `duplicate phrase "${phrase}" — an earlier line already defines it`,
            });
            return;
        }
        seen.add(dedupeKey);

        const action = parsed.action;
        commands.push({
            id: `user.${id++}`,
            title: `Custom: ${phrase}`,
            phrases: { en: [phrase], ka: [phrase] },
            risk: 'medium',
            context: 'global',
            requiresConfirmation: false,
            capturesQuery: false,
            literal: true,
            action: () => action,
            appScope,
        });
    });

    // App-scoped commands first: in the matcher's literal step the
    // first phrase match wins, so a scoped command must out-rank a
    // same-phrase global one when its app is focused.
    commands.sort((a, b) => (a.appScope ? 0 : 1) - (b.appScope ? 0 : 1));
    return { commands, errors };
}

// ─── Registry accessors (consumed by commandRegistry) ────────────────

/** The user-defined commands — folded into `buildVoiceCommandRegistry`.
 *  Synchronous: returns the last loaded snapshot. */
export function getUserVoiceCommands(): VoiceCommandDefinition[] {
    return snapshot.commands;
}

/** True when any user command is app-scoped — lets `executeVoiceCommand`
 *  skip the foreground-app lookup entirely when nobody uses scoping. */
export function userCommandsHaveAppScopes(): boolean {
    return snapshot.commands.some((command) => command.appScope !== undefined);
}

// ─── Loading + hot-reload ────────────────────────────────────────────

/** Read + parse the command file, updating the snapshot and store. */
export async function loadUserVoiceCommands(): Promise<void> {
    let text = '';
    try {
        text = await invoke<string>('voice_read_user_commands');
    } catch (error) {
        console.warn('voice: could not read the user command file:', error);
        return;
    }
    const { commands, errors } = parseUserVoiceCommands(text);
    snapshot = { commands, errors, loaded: true };
    store.set(snapshot);
}

let initialized = false;
let unlistenChanged: UnlistenFn | null = null;
let reloadTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * Wire user voice commands for THIS window: start the backend file
 * watcher (idempotent), load the file once, and hot-reload on change.
 * Call once per window that matches voice commands. Returns a cleanup.
 */
export function initUserVoiceCommands(): () => void {
    if (initialized) return () => {};
    initialized = true;

    void (async () => {
        try {
            await invoke('voice_watch_user_commands');
        } catch (error) {
            console.warn('voice: could not start the command-file watcher:', error);
        }
        await loadUserVoiceCommands();
        try {
            unlistenChanged = await listen('voice-user-commands-changed', () => {
                // Debounce — one file save can fire several fs events.
                if (reloadTimer) clearTimeout(reloadTimer);
                reloadTimer = setTimeout(() => {
                    reloadTimer = null;
                    void loadUserVoiceCommands();
                }, 250);
            });
        } catch (error) {
            console.warn('voice: could not wire command-file hot-reload:', error);
        }
    })();

    return () => {
        unlistenChanged?.();
        unlistenChanged = null;
        if (reloadTimer) {
            clearTimeout(reloadTimer);
            reloadTimer = null;
        }
        initialized = false;
    };
}
