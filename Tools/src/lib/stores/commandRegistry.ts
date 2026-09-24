/*
  commandRegistry — the declarative voice-command registry.

  Voice upgrade, Layer 5. Replaces the old hardcoded `voiceCommand.ts`
  matcher. Every voice command is now a `VoiceCommandDefinition` —
  declarative data (id, title, bilingual trigger phrases, risk tier,
  context, confirmation policy) plus a pure `action` builder — it
  returns a declarative `VoiceAction`; the side effects live in
  `voiceActionExecutor` (#15), never here. The registry is the SINGLE
  SOURCE OF TRUTH for:
    - the matcher  — `matchVoiceCommand` / `executeVoiceCommand`,
    - the grammar  — `buildCommandGrammar` (the Vosk command-mode
      vocabulary) is derived from the registry, so it can never drift
      from the actual command set.

  Commands come from four "modules":
    - overlay/navigation — the three global overlays, authored here.
    - the launcher       — apps & sites (the `APPS` table).
    - tools              — every installed KeepItLocal tool, generated
                           from the live tool list.
    - keyboard / mouse   — voice OS-input commands (#18a): `literal`
                           commands matched as the whole spoken phrase.

  Bilingual: every authored command carries `phrases.en` + `phrases.ka`,
  and the matcher / grammar use the set for the active locale. The
  generated tool/app commands derive phrases from the live (current-
  locale) data. Georgian phrasing here is a sensible starting point —
  tune it against real `ka`-model output.

  TypeScript, not Rust: see the note in transcriptNormalizer.ts — the
  whole command pipeline is TS, and #12–#16 evolve these same modules.

  Risk / context / confirmation fields are now ENFORCED:
  `matchVoiceCommand` tags every match `exact` or `fuzzy`, and
  `executeVoiceCommand` routes it through the `voiceSafetyGate` (#14) —
  fuzzy guesses of risky commands are refused, dangerous commands are
  parked for a spoken "confirm". Context is filtered by the #12
  resolver. The keyboard / mouse commands (#18a) are the first MEDIUM-
  risk tier — exact-match-only, never confirmed; dangerous commands
  arrive later.
*/

import { get } from 'svelte/store';
import { toast } from './toasts';
import { settings, type Locale } from './settings';
import type { Tool } from '$lib/tools';
import type { VoiceSurface, VoiceContextSnapshot } from './voiceContext';
import type { VoiceAction, FrecencyMark } from './voiceActionExecutor';
import {
    runThroughGate,
    resolvePendingConfirmation,
    confirmationGrammarPhrases,
    type GateOutcome,
} from './voiceSafetyGate';
import { invoke } from '@tauri-apps/api/core';
import { getUserVoiceCommands, userCommandsHaveAppScopes } from './userVoiceCommands';
import { getCommandPhraseOverrides } from './commandOverrides';

// ─── App / site registry (the launcher module's command data) ────────
//
// Each entry describes how to open a thing the user asks for by name.
// `url` opens with no query; `searchUrl` builds a deep-link with a
// query ("open youtube search cats"); `exeId` launches a native app.

type AppEntry = {
    url?: string;
    searchUrl?: (q: string) => string;
    exeId?: string;
    label: string;
};

const APPS: Record<string, AppEntry> = {
    google: {
        url: 'https://www.google.com',
        searchUrl: (q) => `https://www.google.com/search?q=${encodeURIComponent(q)}`,
        label: 'Google',
    },
    youtube: {
        url: 'https://www.youtube.com',
        searchUrl: (q) =>
            `https://www.youtube.com/results?search_query=${encodeURIComponent(q)}`,
        label: 'YouTube',
    },
    bing: {
        url: 'https://www.bing.com',
        searchUrl: (q) => `https://www.bing.com/search?q=${encodeURIComponent(q)}`,
        label: 'Bing',
    },
    duckduckgo: {
        url: 'https://duckduckgo.com',
        searchUrl: (q) => `https://duckduckgo.com/?q=${encodeURIComponent(q)}`,
        label: 'DuckDuckGo',
    },
    github: {
        url: 'https://github.com',
        searchUrl: (q) => `https://github.com/search?q=${encodeURIComponent(q)}`,
        label: 'GitHub',
    },
    wikipedia: {
        url: 'https://www.wikipedia.org',
        searchUrl: (q) =>
            `https://en.wikipedia.org/wiki/Special:Search?search=${encodeURIComponent(q)}`,
        label: 'Wikipedia',
    },
    stackoverflow: {
        url: 'https://stackoverflow.com',
        searchUrl: (q) => `https://stackoverflow.com/search?q=${encodeURIComponent(q)}`,
        label: 'Stack Overflow',
    },
    reddit: {
        url: 'https://www.reddit.com',
        searchUrl: (q) => `https://www.reddit.com/search/?q=${encodeURIComponent(q)}`,
        label: 'Reddit',
    },
    twitter: {
        url: 'https://twitter.com',
        searchUrl: (q) => `https://twitter.com/search?q=${encodeURIComponent(q)}`,
        label: 'Twitter / X',
    },
    amazon: {
        url: 'https://www.amazon.com',
        searchUrl: (q) => `https://www.amazon.com/s?k=${encodeURIComponent(q)}`,
        label: 'Amazon',
    },
    maps: {
        url: 'https://www.google.com/maps',
        searchUrl: (q) => `https://www.google.com/maps/search/${encodeURIComponent(q)}`,
        label: 'Google Maps',
    },
    gmail: { url: 'https://mail.google.com', label: 'Gmail' },
    calendar: { url: 'https://calendar.google.com', label: 'Google Calendar' },
    drive: { url: 'https://drive.google.com', label: 'Google Drive' },
    facebook: { url: 'https://www.facebook.com', label: 'Facebook' },
    instagram: { url: 'https://www.instagram.com', label: 'Instagram' },
    linkedin: { url: 'https://www.linkedin.com', label: 'LinkedIn' },
    netflix: { url: 'https://www.netflix.com', label: 'Netflix' },
    spotify: { url: 'https://open.spotify.com', label: 'Spotify' },
    discord: { url: 'https://discord.com/app', label: 'Discord' },
    whatsapp: { url: 'https://web.whatsapp.com', label: 'WhatsApp Web' },
    chatgpt: { url: 'https://chat.openai.com', label: 'ChatGPT' },
    claude: { url: 'https://claude.ai', label: 'Claude' },
    notepad: { exeId: 'notepad', label: 'Notepad' },
    calculator: { exeId: 'calc', label: 'Calculator' },
    explorer: { exeId: 'explorer', label: 'File Explorer' },
    cmd: { exeId: 'cmd', label: 'Command Prompt' },
    terminal: { exeId: 'cmd', label: 'Terminal' },
    powershell: { exeId: 'powershell', label: 'PowerShell' },
    taskmanager: { exeId: 'taskmgr', label: 'Task Manager' },
    'task manager': { exeId: 'taskmgr', label: 'Task Manager' },
    chrome: {
        exeId: 'chrome',
        url: 'https://www.google.com',
        searchUrl: (q) => `https://www.google.com/search?q=${encodeURIComponent(q)}`,
        label: 'Google Chrome',
    },
    firefox: {
        exeId: 'firefox',
        url: 'https://www.google.com',
        searchUrl: (q) => `https://www.google.com/search?q=${encodeURIComponent(q)}`,
        label: 'Mozilla Firefox',
    },
    edge: {
        exeId: 'msedge',
        url: 'https://www.bing.com',
        searchUrl: (q) => `https://www.bing.com/search?q=${encodeURIComponent(q)}`,
        label: 'Microsoft Edge',
    },
};
// Spoken-form aliases — Vosk transcribes the way people speak, not the
// way code is named. Each alias shares the canonical entry by reference.
APPS['google chrome'] = APPS.chrome;
APPS['mozilla firefox'] = APPS.firefox;
APPS['fire fox'] = APPS.firefox;
APPS['mozilla'] = APPS.firefox;
APPS['microsoft edge'] = APPS.edge;
APPS['ms edge'] = APPS.edge;
APPS['g mail'] = APPS.gmail;
APPS['you tube'] = APPS.youtube;
APPS['whats app'] = APPS.whatsapp;
APPS['chat gpt'] = APPS.chatgpt;
APPS['stack overflow'] = APPS.stackoverflow;
APPS['duck duck go'] = APPS.duckduckgo;
APPS['google maps'] = APPS.maps;
APPS['google calendar'] = APPS.calendar;
APPS['google drive'] = APPS.drive;

// ─── Registry types ──────────────────────────────────────────────────

export type CommandRisk = 'safe' | 'medium' | 'dangerous';

/** Where a command is valid — checked against the active
 *  `VoiceContextSnapshot` by `commandInContext`. 'global' commands are
 *  available everywhere; a `VoiceSurface` value scopes the command to
 *  that KeepItLocal surface. Every command today is 'global'; scoped
 *  commands populate this as their per-surface features land. */
export type CommandContext = 'global' | VoiceSurface;

export interface LocalizedPhrases {
    en: string[];
    ka: string[];
}

export interface VoiceCommandArgs {
    /** Captured slot text — the query for "search <query>". Empty when
     *  the command takes no query. */
    query: string;
}

export interface VoiceCommandDefinition {
    /** Stable id, e.g. "overlay.clipboard", "app.chrome", "tool.hash-check". */
    id: string;
    /** Human-readable label for toasts / the activity log. */
    title: string;
    /** Spoken trigger phrases per locale — the remainder AFTER the
     *  "open"/"launch"/… verb (e.g. "clipboard", not "open clipboard"). */
    phrases: LocalizedPhrases;
    /** Risk tier — consumed by the #14 safety gate. */
    risk: CommandRisk;
    /** Where the command is valid — consumed by the #12 context resolver. */
    context: CommandContext;
    /** Whether #14's safety gate must confirm before running. */
    requiresConfirmation: boolean;
    /** True for the web-search command — trailing text becomes the
     *  query slot rather than part of the trigger phrase. */
    capturesQuery: boolean;
    /** Build the declarative action this command performs. Pure —
     *  given the captured args it returns a `VoiceAction`; the side
     *  effects happen in `executeAction` (#15), never in this builder. */
    action: (args: VoiceCommandArgs) => VoiceAction;
    /** True for keyboard / mouse commands (#18a) whose `phrases` are
     *  COMPLETE spoken phrases ("press enter", "click") — matched
     *  against the whole transcript with no verb stripped, and emitted
     *  into the grammar verbatim. Absent ⇒ a verb-phrase command (a
     *  verb is stripped, then the remainder matched). */
    literal?: boolean;
    /** Per-app scope for a user-authored command (#21) — the foreground
     *  process name (e.g. "chrome") this command requires. The matcher
     *  only accepts it when that app is focused. Absent ⇒ global. Set
     *  ONLY by `userVoiceCommands`; every built-in command is global. */
    appScope?: string;
}

/** How confidently the matcher resolved a command. `exact` — the
 *  transcript named the command (an overlay phrase, an app name, a
 *  search verb); `fuzzy` — a token-overlap guess against the installed
 *  tools. The safety gate refuses a `fuzzy` match of any command
 *  riskier than `safe`. */
export type CommandMatchKind = 'exact' | 'fuzzy';

/** A matched command + any captured slot text. */
export interface VoiceCommandMatch {
    definition: VoiceCommandDefinition;
    query: string;
    /** How the matcher arrived at this command — gates risky commands. */
    matchKind: CommandMatchKind;
}

/** The result of matching a transcript against the registry:
 *   · `match`     — one confident command (see `VoiceCommandMatch`).
 *   · `ambiguous` — two-plus equally-good fuzzy candidates; refused so
 *                   the user can name the one they meant.
 *   · `none`      — nothing recognized. */
export type VoiceCommandMatchResult =
    | { kind: 'match'; match: VoiceCommandMatch }
    | { kind: 'ambiguous'; candidates: string[] }
    | { kind: 'none' };

/** The precise result of `executeVoiceCommand`, beyond matched / not —
 *  #16's feedback layer switches on this. */
export type VoiceCommandStatus =
    | 'executed' // a command ran immediately
    | 'confirmed' // a parked command was confirmed and ran
    | 'cancelled' // a parked command was cancelled
    | 'confirmation-required' // a dangerous command is parked, awaiting "confirm"
    | 'blocked' // refused by the safety gate (a fuzzy guess of a risky command)
    | 'ambiguous' // matched 2+ equal candidates — the user must be specific
    | 'cooldown' // a duplicate of the just-run command, swallowed
    | 'no-match'; // nothing recognized

export type VoiceCommandOutcome = {
    /** True iff a command actually ran (executed or confirmed). */
    matched: boolean;
    /** The precise gate / parse result. */
    status: VoiceCommandStatus;
    /** Short label describing what happened — for toasts / activity log. */
    description?: string;
    /** True when the transcript looked like a command attempt but no
     *  command ran — lets the voice overlay say "didn't understand"
     *  instead of pasting the words as dictation. */
    looksLikeCommand?: boolean;
};

// ─── Locale-aware vocabulary ─────────────────────────────────────────

/** Verbs that introduce an "open X" command, longest-phrase first.
 *  Stripped by the matcher to leave the trigger remainder. */
const OPEN_VERBS: Record<Locale, string[]> = {
    en: ['go to', 'show me', 'open', 'launch', 'start', 'show'],
    ka: ['გახსენი', 'გახსნა', 'გაუშვი', 'მაჩვენე'],
};

/** Verbs that introduce a "search <query>" command. */
const SEARCH_VERBS: Record<Locale, string[]> = {
    en: ['search for', 'search', 'look up', 'lookup', 'find me', 'find', 'google'],
    ka: ['მოძებნე', 'ძებნა'],
};

/** Connector words between an app and its in-app search query
 *  ("open youtube search cats", "google for rust"). */
const SEARCH_CONNECTORS: Record<Locale, string[]> = {
    en: ['search for', 'search', 'find', 'lookup', 'look up', 'for'],
    ka: ['მოძებნე', 'ძებნა'],
};

/** The verb the Vosk command grammar prepends to every phrase. */
const GRAMMAR_OPEN_VERB: Record<Locale, string> = { en: 'open', ka: 'გახსენი' };

// ─── Static commands (the overlay / navigation + search module) ──────

const OVERLAY_COMMANDS: VoiceCommandDefinition[] = [
    {
        id: 'overlay.clipboard',
        title: 'Opening clipboard history',
        phrases: {
            en: ['clipboard', 'clipboard history'],
            ka: ['ბუფერი', 'ბუფერის ისტორია'],
        },
        risk: 'safe',
        context: 'global',
        requiresConfirmation: false,
        capturesQuery: false,
        action: () => ({ kind: 'open-overlay', overlay: 'clipboard' }),
    },
    {
        id: 'overlay.search',
        title: 'Opening search',
        phrases: {
            en: ['search', 'search overlay', 'launcher'],
            ka: ['ძებნა', 'მაძიებელი'],
        },
        risk: 'safe',
        context: 'global',
        requiresConfirmation: false,
        capturesQuery: false,
        action: () => ({ kind: 'open-overlay', overlay: 'search' }),
    },
    {
        id: 'overlay.voice',
        title: 'Opening voice',
        phrases: { en: ['voice', 'voice overlay'], ka: ['ხმა'] },
        risk: 'safe',
        context: 'global',
        requiresConfirmation: false,
        capturesQuery: false,
        action: () => ({ kind: 'open-overlay', overlay: 'voice' }),
    },
];

/** The bare web-search command — "search <query>" → the default
 *  engine. `capturesQuery` so the matcher captures the trailing text. */
const SEARCH_COMMAND: VoiceCommandDefinition = {
    id: 'search.web',
    title: 'Web search',
    // Triggered by the SEARCH_VERBS, not by an "open <phrase>" — phrases
    // is empty; the matcher routes search-verb utterances here directly.
    phrases: { en: [], ka: [] },
    risk: 'safe',
    context: 'global',
    requiresConfirmation: false,
    capturesQuery: true,
    action: (args) => appAction(APPS.google, args.query || null),
};

// ─── Command action builders ─────────────────────────────────────────

/** Resolve an `AppEntry` + optional query into a declarative action,
 *  mirroring the launcher's precedence: an explicit query opens the
 *  app's in-app search; a native app launches (falling back to its web
 *  version when not installed); a pure web app opens its homepage.
 *  Pure — the side effects happen later, in `executeAction`. */
function appAction(
    entry: AppEntry,
    query: string | null,
    frecency?: FrecencyMark,
): VoiceAction {
    // An explicit query is the strongest intent signal — use the
    // search URL regardless of which browser is installed.
    if (query && entry.searchUrl) {
        return { kind: 'open-url', url: entry.searchUrl(query), frecency };
    }
    if (entry.exeId) {
        return {
            kind: 'launch-app',
            exeId: entry.exeId,
            label: entry.label,
            fallbackUrl: entry.url,
            frecency,
        };
    }
    if (entry.url) {
        return { kind: 'open-url', url: entry.url, frecency };
    }
    return { kind: 'noop' };
}

// ─── Generated commands (apps + tools) ───────────────────────────────

/** One command definition per `APPS` key (aliases included — each
 *  spoken form is its own trigger). The launcher module's commands. */
function appCommandDefinitions(): VoiceCommandDefinition[] {
    return Object.keys(APPS).map((key) => {
        const entry = APPS[key];
        return {
            id: `app.${key.replace(/\s+/g, '-')}`,
            title: `Opening ${entry.label}`,
            // App / brand names are not localized — the same key works
            // as the trigger remainder in either language.
            phrases: { en: [key], ka: [key] },
            risk: 'safe',
            context: 'global',
            requiresConfirmation: false,
            capturesQuery: Boolean(entry.searchUrl),
            action: (args) =>
                appAction(entry, args.query || null, { kind: 'app', path: key }),
        } satisfies VoiceCommandDefinition;
    });
}

/** One command definition per available installed tool — the tools
 *  module's commands. `tool.name` is the current-locale display name. */
function toolCommandDefinitions(tools: Tool[]): VoiceCommandDefinition[] {
    return tools
        .filter((tool) => tool.available)
        .map((tool) => {
            const name = tool.name.toLowerCase();
            return {
                id: `tool.${tool.id}`,
                title: `Opening ${tool.name}`,
                phrases: { en: [name], ka: [name] },
                risk: 'safe',
                context: 'global',
                requiresConfirmation: false,
                capturesQuery: false,
                action: () => ({ kind: 'open-tool', toolId: tool.id }),
            } satisfies VoiceCommandDefinition;
        });
}

// ─── Keyboard & mouse commands (#18a — voice OS input) ───────────────
//
// `literal` commands: their `phrases` are complete spoken phrases
// ("press enter", "copy", "scroll down"), matched against the whole
// transcript with no verb stripped. They emulate real input via the
// Win32 `SendInput` backend (voiceActionExecutor → voice_input.rs).
// All Medium risk — the #14 gate runs them on an exact match only
// (never a fuzzy guess) and never asks for confirmation.
//
// The Vosk command grammar is a flat phrase list, so the set is an
// explicit curated list (named keys, F-keys, common shortcuts, mouse
// actions) rather than an open "press <anything>" slot.

/** Pixels the cursor nudges per "move mouse <direction>". Coarse by
 *  design — precise pointing is the #18b mouse grid. */
const MOUSE_NUDGE_PX = 40;

/** Keyboard commands: [spoken phrase, key name, modifier names]. The
 *  shortcut phrases ("copy", "save", …) are chosen to never collide
 *  with an open/search verb. */
const KEYSTROKE_COMMANDS: ReadonlyArray<readonly [string, string, string[]]> = [
    ['press enter', 'enter', []],
    ['press escape', 'escape', []],
    ['press tab', 'tab', []],
    ['press space', 'space', []],
    ['press backspace', 'backspace', []],
    ['press delete', 'delete', []],
    ['press up', 'up', []],
    ['press down', 'down', []],
    ['press left', 'left', []],
    ['press right', 'right', []],
    ['press home', 'home', []],
    ['press end', 'end', []],
    ['press page up', 'page up', []],
    ['press page down', 'page down', []],
    ['copy', 'c', ['ctrl']],
    ['cut', 'x', ['ctrl']],
    ['paste', 'v', ['ctrl']],
    ['undo', 'z', ['ctrl']],
    ['redo', 'y', ['ctrl']],
    ['select all', 'a', ['ctrl']],
    ['save', 's', ['ctrl']],
];

/** Mouse commands: [spoken phrase, the action it performs]. */
const MOUSE_COMMANDS: ReadonlyArray<readonly [string, VoiceAction]> = [
    ['click', { kind: 'mouse-click', button: 'left', double: false }],
    ['double click', { kind: 'mouse-click', button: 'left', double: true }],
    ['right click', { kind: 'mouse-click', button: 'right', double: false }],
    ['middle click', { kind: 'mouse-click', button: 'middle', double: false }],
    ['scroll up', { kind: 'mouse-scroll', direction: 'up', notches: 5 }],
    ['scroll down', { kind: 'mouse-scroll', direction: 'down', notches: 5 }],
    ['move mouse up', { kind: 'mouse-move', dx: 0, dy: -MOUSE_NUDGE_PX }],
    ['move mouse down', { kind: 'mouse-move', dx: 0, dy: MOUSE_NUDGE_PX }],
    ['move mouse left', { kind: 'mouse-move', dx: -MOUSE_NUDGE_PX, dy: 0 }],
    ['move mouse right', { kind: 'mouse-move', dx: MOUSE_NUDGE_PX, dy: 0 }],
];

/** The phonetic alphabet (#18c) — one spoken word per letter, for
 *  precise character entry. The Talon set: short, acoustically
 *  distinct, common English words a Vosk model recognizes reliably.
 *  Each command types its lowercase letter via a `keystroke` action. */
const PHONETIC_ALPHABET: ReadonlyArray<readonly [string, string]> = [
    ['air', 'a'], ['bat', 'b'], ['cap', 'c'], ['drum', 'd'], ['each', 'e'],
    ['fine', 'f'], ['gust', 'g'], ['harp', 'h'], ['sit', 'i'], ['jury', 'j'],
    ['crunch', 'k'], ['look', 'l'], ['made', 'm'], ['near', 'n'], ['odd', 'o'],
    ['pit', 'p'], ['quench', 'q'], ['red', 'r'], ['sun', 's'], ['trap', 't'],
    ['urge', 'u'], ['vest', 'v'], ['whale', 'w'], ['plex', 'x'], ['yank', 'y'],
    ['zip', 'z'],
];

/** Window-control commands (#19): [spoken phrase, the action]. The
 *  foreground-window ops route through the `voice_window_action`
 *  backend; "switch window" is just an Alt+Tab keystroke (18a). */
const WINDOW_COMMANDS: ReadonlyArray<readonly [string, VoiceAction]> = [
    ['minimize window', { kind: 'window-action', action: 'minimize' }],
    ['maximize window', { kind: 'window-action', action: 'maximize' }],
    ['restore window', { kind: 'window-action', action: 'restore' }],
    ['close window', { kind: 'window-action', action: 'close' }],
    ['snap left', { kind: 'window-action', action: 'snap-left' }],
    ['snap right', { kind: 'window-action', action: 'snap-right' }],
    ['center window', { kind: 'window-action', action: 'center' }],
    ['switch window', { kind: 'keystroke', key: 'tab', modifiers: ['alt'] }],
];

/** Title-case a spoken phrase for the command's display title. */
function titleCasePhrase(phrase: string): string {
    return phrase.replace(/\b\w/g, (c) => c.toUpperCase());
}

/**
 * One `VoiceCommandDefinition` per keyboard / mouse command (#18a).
 * Literal, Medium-risk commands — `phrases` is the complete spoken
 * phrase; the #14 gate runs them on an exact match, no confirmation.
 * `ka` mirrors `en` for now (the key/action vocabulary is not yet
 * localized — tune against the `ka` model with #22 / #26).
 */
function keyboardMouseCommandDefinitions(): VoiceCommandDefinition[] {
    const literalCommand = (
        id: string,
        phrase: string,
        action: (args: VoiceCommandArgs) => VoiceAction,
        title?: string,
    ): VoiceCommandDefinition => ({
        id,
        title: title ?? titleCasePhrase(phrase),
        phrases: { en: [phrase], ka: [phrase] },
        risk: 'medium',
        context: 'global',
        requiresConfirmation: false,
        capturesQuery: false,
        literal: true,
        action,
    });

    const defs: VoiceCommandDefinition[] = [];
    for (const [phrase, key, modifiers] of KEYSTROKE_COMMANDS) {
        defs.push(
            literalCommand(`input.key.${phrase.replace(/\s+/g, '-')}`, phrase, () => ({
                kind: 'keystroke',
                key,
                modifiers,
            })),
        );
    }
    for (let n = 1; n <= 12; n++) {
        defs.push(
            literalCommand(`input.key.f${n}`, `press f${n}`, () => ({
                kind: 'keystroke',
                key: `f${n}`,
                modifiers: [],
            })),
        );
    }
    // Phonetic alphabet (#18c) — one spoken word per letter.
    for (const [word, letter] of PHONETIC_ALPHABET) {
        defs.push(
            literalCommand(
                `input.letter.${letter}`,
                word,
                () => ({ kind: 'keystroke', key: letter, modifiers: [] }),
                `Type "${letter}"`,
            ),
        );
    }
    for (const [phrase, action] of MOUSE_COMMANDS) {
        defs.push(
            literalCommand(
                `input.mouse.${phrase.replace(/\s+/g, '-')}`,
                phrase,
                () => action,
            ),
        );
    }
    // The mouse-grid overlay (#18b) — opens the hands-free pointer grid.
    defs.push(
        literalCommand('input.mouse-grid', 'mouse grid', () => ({
            kind: 'open-mouse-grid',
        })),
    );
    // The accessibility element overlay (#20) — numbers every clickable
    // control in the focused window so it can be picked by voice.
    defs.push(
        literalCommand('input.ui-elements', 'show elements', () => ({
            kind: 'open-ui-elements',
        })),
    );
    // Window control (#19) — foreground-window ops + Alt+Tab.
    for (const [phrase, action] of WINDOW_COMMANDS) {
        defs.push(
            literalCommand(
                `input.window.${phrase.replace(/\s+/g, '-')}`,
                phrase,
                () => action,
            ),
        );
    }
    return defs;
}

/**
 * Build the full command registry for the current state. Pure — the
 * matcher and the grammar builder both call this so they can never
 * disagree about what commands exist.
 */
export function buildVoiceCommandRegistry(tools: Tool[]): VoiceCommandDefinition[] {
    const overrides = getCommandPhraseOverrides();
    // A user override (Settings → Voice editor) replaces a built-in command's
    // phrases. No-op for any command id without an override — so app/tool/user
    // commands pass through untouched.
    const applyOverride = (def: VoiceCommandDefinition): VoiceCommandDefinition => {
        const ov = overrides[def.id];
        if (!ov) return def;
        return {
            ...def,
            phrases: { en: ov.en ?? def.phrases.en, ka: ov.ka ?? def.phrases.ka },
        };
    };
    return [
        ...OVERLAY_COMMANDS,
        SEARCH_COMMAND,
        ...appCommandDefinitions(),
        ...toolCommandDefinitions(tools),
        ...keyboardMouseCommandDefinitions(),
        // User-authored commands (#21) come last: a built-in literal
        // command therefore wins a phrase collision with a user one.
        ...getUserVoiceCommands(),
    ].map(applyOverride);
}

/** A curated built-in command exposed to the Settings editor for phrase
 *  customization. `defaults` are the shipped phrases — what "Reset" restores. */
export interface CuratedBuiltinCommand {
    id: string;
    title: string;
    /** Editor group: navigation | keyboard | letters | mouse | window | other. */
    category: string;
    defaults: LocalizedPhrases;
}

/** The built-in commands the user may re-phrase in Settings → Voice. Excludes
 *  the dynamic app/tool commands (those track live names) and the query-only
 *  web-search command (it has no fixed phrase). */
export function getCuratedBuiltinCommands(): CuratedBuiltinCommand[] {
    const out: CuratedBuiltinCommand[] = [];
    for (const def of OVERLAY_COMMANDS) {
        out.push({ id: def.id, title: def.title, category: 'navigation', defaults: def.phrases });
    }
    for (const def of keyboardMouseCommandDefinitions()) {
        let category = 'other';
        if (def.id.startsWith('input.key')) category = 'keyboard';
        else if (def.id.startsWith('input.letter')) category = 'letters';
        else if (def.id.startsWith('input.mouse')) category = 'mouse';
        else if (def.id.startsWith('input.window')) category = 'window';
        out.push({ id: def.id, title: def.title, category, defaults: def.phrases });
    }
    return out;
}

/** Whether `def` is available in the given context. With no context,
 *  only 'global' commands are available — the safe default. The grammar
 *  builder and the matcher both gate on this, so a context-scoped
 *  command appears in the recognizer's vocabulary AND is matchable
 *  exactly when its surface is active. */
function commandInContext(
    def: VoiceCommandDefinition,
    context: VoiceContextSnapshot | undefined,
): boolean {
    if (def.context === 'global') return true;
    return context ? def.context === context.surface : false;
}

// ─── Grammar ─────────────────────────────────────────────────────────

/**
 * Build the flat phrase list that constrains the Vosk recognizer in
 * command mode — derived entirely from the registry, so the grammar
 * can never drift from the actual command handlers. Each non-query
 * command contributes "<open-verb> <phrase>" for the active locale.
 *
 * `context` filters the registry: only commands valid in the active
 * context reach the grammar (no context → global commands only).
 */
export function buildCommandGrammar(
    tools: Tool[],
    context?: VoiceContextSnapshot,
): string[] {
    const locale = voiceModelLocale();
    const verb = GRAMMAR_OPEN_VERB[locale];
    const phrases = new Set<string>();
    for (const def of buildVoiceCommandRegistry(tools)) {
        // The web-search command's query slot can't be grammar-
        // constrained — command mode reaches search via "open search".
        if (def.capturesQuery) continue;
        if (!commandInContext(def, context)) continue;
        for (const phrase of def.phrases[locale]) {
            // Literal commands (keyboard / mouse, #18a) ARE the complete
            // spoken phrase; verb-phrase commands get the open-verb.
            phrases.add(def.literal ? phrase : `${verb} ${phrase}`);
        }
    }
    // The safety gate parks dangerous commands for a spoken "confirm" /
    // "cancel" — those words must be in the grammar or the recognizer
    // would never hear them. Always present: the grammar is fixed for
    // the session, so it can't be extended once a command is parked.
    for (const word of confirmationGrammarPhrases(locale)) {
        phrases.add(word);
    }
    return [...phrases];
}

// ─── Matcher ─────────────────────────────────────────────────────────

/**
 * The language of a Vosk model, derived from its folder name. Vosk
 * model folders follow `vosk-model-[small-]<lang>[-region]-<version>`
 * (e.g. `vosk-model-small-en-us-0.15`, `vosk-model-small-ka-0.22`), so
 * the language token sits in the basename. Defaults to `en` for an
 * empty path or a non-conventional folder name (the safe fallback).
 */
function resolveModelLanguage(modelPath: string): Locale {
    const basename = (modelPath.split(/[\\/]/).pop() ?? '').toLowerCase();
    return basename.split(/[-_.]+/).includes('ka') ? 'ka' : 'en';
}

/**
 * The locale the command grammar + matcher operate in — the loaded
 * Vosk model's language, NOT the UI locale (`settings.locale`). The
 * grammar fed to Vosk and the phrases the matcher compares against MUST
 * match the model: a Georgian model recognizes only Georgian, so
 * feeding it English phrases would recognize nothing. (#13)
 */
export function voiceModelLocale(): Locale {
    return resolveModelLanguage(get(settings).voskModelPath ?? '');
}

/** Strip any of the given leading phrases (longest-first). Returns the
 *  trimmed remainder, or `null` if none of them prefix the text. */
function stripLeading(text: string, prefixes: string[]): string | null {
    const ordered = [...prefixes].sort((a, b) => b.length - a.length);
    for (const p of ordered) {
        if (text === p) return '';
        if (text.startsWith(`${p} `)) return text.slice(p.length + 1).trim();
    }
    return null;
}

/** Find an overlay/navigation command whose trigger phrase exactly
 *  equals `remainder` (the text after the open-verb). */
function matchExactPhrase(
    remainder: string,
    registry: VoiceCommandDefinition[],
    locale: Locale,
): VoiceCommandDefinition | null {
    for (const def of registry) {
        // Literal commands (#18a) are matched as the whole transcript
        // by `matchVoiceCommand`, never as an "open <phrase>" remainder.
        if (def.capturesQuery || def.literal) continue;
        if (def.phrases[locale].includes(remainder)) return def;
    }
    return null;
}

/** The outcome of fuzzy tool matching — a definite tool, an ambiguous
 *  tie between distinct tools (refused: too risky to guess), or no
 *  overlap at all. */
type ToolMatchResult =
    | { kind: 'match'; definition: VoiceCommandDefinition }
    | { kind: 'ambiguous'; candidates: string[] }
    | { kind: 'none' };

/** Fuzzy token-overlap match of `remainder` against the installed
 *  tools — "open hash" → Hash Check. Frecency breaks ties but never
 *  promotes a zero-overlap tool. When two distinct tools finish tied
 *  at the top score — frecency could not separate them — the result is
 *  `ambiguous` rather than an arbitrary pick. */
function matchToolDefinition(
    remainder: string,
    tools: Tool[],
    registry: VoiceCommandDefinition[],
    frecency: Record<string, number>,
): ToolMatchResult {
    const tokens = remainder
        .split(/\s+/)
        .filter((t) => t.length >= 2);
    if (tokens.length === 0) return { kind: 'none' };

    let bestScore = 0;
    let bestTool: Tool | null = null;
    /** A distinct tool tied with `bestTool` at `bestScore`. If one
     *  survives to the end, the fuzzy match is ambiguous. */
    let tiedTool: Tool | null = null;
    for (const tool of tools) {
        if (!tool.available) continue;
        const haystack = `${tool.name} ${tool.id} ${tool.description}`.toLowerCase();
        let base = 0;
        for (const tok of tokens) {
            if (haystack.includes(tok)) base += tok.length;
            if (tool.name.toLowerCase().split(/\s+/).includes(tok)) base += 5;
            if (tool.id.toLowerCase() === tok) base += 50;
        }
        if (tool.name.toLowerCase() === remainder) base += 100;
        // Frecency only applies once a real token match exists.
        const score = base >= 5 ? base + (frecency[tool.id] ?? 0) * 5 : base;
        if (score > bestScore) {
            bestScore = score;
            bestTool = tool;
            tiedTool = null;
        } else if (score === bestScore && bestScore >= 5 && tool !== bestTool) {
            tiedTool = tool;
        }
    }
    if (bestScore < 5 || !bestTool) return { kind: 'none' };
    // A genuine tie between two tools is not safe to guess from — ask
    // the user to name the one they meant.
    if (tiedTool) {
        return { kind: 'ambiguous', candidates: [bestTool.name, tiedTool.name] };
    }
    const def = registry.find((d) => d.id === `tool.${bestTool!.id}`);
    return def ? { kind: 'match', definition: def } : { kind: 'none' };
}

/** Look for "<app> [and] <connector> <query>" — e.g. "youtube search
 *  cats", "google for rust async". The app must lead; a search-capable
 *  app entry + a non-empty query is required. */
function matchAppWithSearch(
    remainder: string,
    registry: VoiceCommandDefinition[],
    locale: Locale,
): VoiceCommandMatch | null {
    // Longest app key first so "task manager" beats "task".
    const appKeys = Object.keys(APPS).sort(
        (a, b) => b.split(' ').length - a.split(' ').length || b.length - a.length,
    );
    for (const key of appKeys) {
        if (!remainder.startsWith(`${key} `)) continue;
        let rest = remainder.slice(key.length + 1);
        if (rest.startsWith('and ')) rest = rest.slice(4);
        const query = stripLeading(rest, SEARCH_CONNECTORS[locale]);
        if (query === null || query.length === 0) continue;
        if (!APPS[key].searchUrl) continue;
        const def = registry.find((d) => d.id === `app.${key.replace(/\s+/g, '-')}`);
        if (def) return { definition: def, query, matchKind: 'exact' };
    }
    return null;
}

/** Wrap a definite command match. `kind` is the match confidence —
 *  `exact` for a named command, `fuzzy` for a token-overlap guess; the
 *  safety gate treats the two differently. */
function hit(
    definition: VoiceCommandDefinition,
    query: string,
    kind: CommandMatchKind,
): VoiceCommandMatchResult {
    return { kind: 'match', match: { definition, query, matchKind: kind } };
}

/**
 * Match a (normalized) transcript against the registry. Resolution
 * order: bare search → literal keyboard/mouse → "open <overlay>" →
 * "open <app> search <query>" → "open <app>" → "open <tool>" (fuzzy)
 * → bare "<app> <query>".
 *
 * Returns a discriminated result: a confident `match`, an `ambiguous`
 * tie the user must disambiguate, or `none`. Only the fuzzy tool step
 * can produce `ambiguous` — every other step is an exact resolution.
 */
export function matchVoiceCommand(
    transcript: string,
    tools: Tool[],
    options?: {
        context?: VoiceContextSnapshot;
        toolFrecency?: Record<string, number>;
        /** Foreground app (#21) — gates app-scoped user commands. */
        foregroundApp?: string;
    },
): VoiceCommandMatchResult {
    const normalized = transcript.trim().toLowerCase();
    if (normalized.length === 0) return { kind: 'none' };

    const locale = voiceModelLocale();
    // Context filter — the matcher only sees commands valid in the
    // active context (no context → global commands only).
    const registry = buildVoiceCommandRegistry(tools).filter((d) =>
        commandInContext(d, options?.context),
    );
    const frecency = options?.toolFrecency ?? {};

    // 1. Bare "search <query>" → the web-search command.
    const searchQuery = stripLeading(normalized, SEARCH_VERBS[locale]);
    if (searchQuery !== null && searchQuery.length > 0) {
        return hit(SEARCH_COMMAND, searchQuery, 'exact');
    }

    // 1b. Literal commands — keyboard / mouse (#18a) and user-authored
    // commands (#21). Their phrases are complete spoken phrases ("press
    // enter", "save my work"); match the whole transcript exactly, with
    // no verb stripped. An app-scoped user command (#21) matches only
    // when its app is the foreground window; the registry lists scoped
    // commands first so a scoped match out-ranks a same-phrase global.
    for (const def of registry) {
        if (!def.literal || !def.phrases[locale].includes(normalized)) continue;
        if (def.appScope && def.appScope !== (options?.foregroundApp ?? '')) {
            continue;
        }
        return hit(def, '', 'exact');
    }

    // 2. "open <…>" — strip the verb, resolve the remainder.
    const remainder = stripLeading(normalized, OPEN_VERBS[locale]);
    if (remainder !== null && remainder.length > 0) {
        // Overlay commands win over app/tool fuzzy matching.
        const overlay = matchExactPhrase(remainder, registry, locale);
        if (overlay) return hit(overlay, '', 'exact');

        // "<app> search <query>".
        const appSearch = matchAppWithSearch(remainder, registry, locale);
        if (appSearch) return { kind: 'match', match: appSearch };

        // Exact app name.
        const appKey = Object.keys(APPS)
            .sort((a, b) => b.split(' ').length - a.split(' ').length)
            .find((k) => k === remainder);
        if (appKey) {
            const def = registry.find(
                (d) => d.id === `app.${appKey.replace(/\s+/g, '-')}`,
            );
            if (def) return hit(def, '', 'exact');
        }

        // Fuzzy tool match — the one step that can come back ambiguous.
        const toolResult = matchToolDefinition(remainder, tools, registry, frecency);
        if (toolResult.kind === 'match') return hit(toolResult.definition, '', 'fuzzy');
        if (toolResult.kind === 'ambiguous') {
            return { kind: 'ambiguous', candidates: toolResult.candidates };
        }
    }

    // 3. Bare "<app> <query>" shortcut — "youtube cat videos".
    const firstSpace = normalized.indexOf(' ');
    if (firstSpace > 0) {
        const head = normalized.slice(0, firstSpace);
        const tail = normalized.slice(firstSpace + 1).trim();
        const entry = APPS[head];
        if (entry?.searchUrl && tail.length > 0) {
            const def = registry.find(
                (d) => d.id === `app.${head.replace(/\s+/g, '-')}`,
            );
            if (def) return hit(def, tail, 'exact');
        }
    }

    return { kind: 'none' };
}

/** Verb prefixes that signal the user was attempting a command — lets
 *  callers tell "unparsed command" from "free-form dictation". */
function looksLikeCommandText(transcript: string): boolean {
    const t = transcript.trim().toLowerCase();
    const verbs = [
        ...OPEN_VERBS.en,
        ...OPEN_VERBS.ka,
        ...SEARCH_VERBS.en,
        ...SEARCH_VERBS.ka,
    ];
    return verbs.some((v) => t === v || t.startsWith(`${v} `));
}

/** Fold a `GateOutcome` into the `VoiceCommandOutcome` callers expect.
 *  Only `executed` counts as `matched`; every other status is a command
 *  attempt that did NOT run, so `looksLikeCommand` is set to keep the
 *  voice overlay from pasting the spoken words as dictation. */
function mapGateOutcome(
    gate: GateOutcome,
    match: VoiceCommandMatch,
): VoiceCommandOutcome {
    if (gate.status === 'executed') {
        const label = match.query
            ? `${gate.title} — "${match.query}"`
            : gate.title;
        return {
            matched: true,
            status: 'executed',
            // The command ran; surface the executor's failure detail if
            // the action itself missed (e.g. an app that won't launch).
            description: gate.result.success ? label : (gate.result.error ?? label),
        };
    }
    if (gate.status === 'blocked') {
        return {
            matched: false,
            status: 'blocked',
            description: gate.reason,
            looksLikeCommand: true,
        };
    }
    // 'confirmation-required' | 'cooldown' — matched a command, did not
    // run it; the gate has already toasted any user-facing message.
    return {
        matched: false,
        status: gate.status,
        description: gate.title,
        looksLikeCommand: true,
    };
}

/**
 * Match + execute a voice command, end to end. The order matters:
 *
 *   1. A dangerous command parked by the safety gate takes priority —
 *      a bare "confirm" / "cancel" answers it and must not be re-parsed
 *      as a fresh command.
 *   2. Otherwise match the transcript. `none` → callers fall back to
 *      their default (paste as dictation, fill a search box). An
 *      `ambiguous` tie is reported back so the user can be specific.
 *   3. A confident match is run through the `voiceSafetyGate`, which
 *      decides whether it runs now, is parked for confirmation, or is
 *      refused.
 */
export async function executeVoiceCommand(
    transcript: string,
    installedTools: Tool[],
    options?: {
        context?: VoiceContextSnapshot;
        toolFrecency?: Record<string, number>;
        /** Foreground app (#21) — gates app-scoped user commands. */
        foregroundApp?: string;
    },
): Promise<VoiceCommandOutcome> {
    const resolution = await resolvePendingConfirmation(transcript);
    if (resolution) {
        if (resolution.status === 'cancelled') {
            return {
                matched: false,
                status: 'cancelled',
                description: resolution.title,
                looksLikeCommand: true,
            };
        }
        // confirmed — the parked command ran through the executor.
        return {
            matched: true,
            status: 'confirmed',
            description: resolution.result.success
                ? resolution.title
                : (resolution.result.error ?? resolution.title),
            looksLikeCommand: true,
        };
    }

    // #21 — app-scoped user commands need the foreground app to
    // resolve. Look it up only when such commands exist (users without
    // scoping pay nothing) and the caller did not already supply it.
    let matchOptions = options;
    if (userCommandsHaveAppScopes() && !options?.foregroundApp) {
        const foregroundApp = await invoke<string>('voice_get_foreground_app').catch(
            () => '',
        );
        matchOptions = { ...options, foregroundApp };
    }

    const result = matchVoiceCommand(transcript, installedTools, matchOptions);
    if (result.kind === 'none') {
        return {
            matched: false,
            status: 'no-match',
            looksLikeCommand: looksLikeCommandText(transcript),
        };
    }
    if (result.kind === 'ambiguous') {
        toast(
            `Did you mean ${result.candidates
                .map((c) => `"${c}"`)
                .join(' or ')}? Say the full name.`,
            'info',
            4000,
        );
        return {
            matched: false,
            status: 'ambiguous',
            description: `Ambiguous — ${result.candidates.join(', ')}`,
            looksLikeCommand: true,
        };
    }

    // A confident match — the safety gate decides if it runs now.
    return mapGateOutcome(await runThroughGate(result.match), result.match);
}
