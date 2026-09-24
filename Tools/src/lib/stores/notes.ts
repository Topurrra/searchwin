/*
  notes — local note storage over the Rust `*_note(s)` commands.

  Notes are plain `.ki` files (UTF-8 Markdown + optional YAML frontmatter) in
  Documents/KeepItLocal Notes. This module owns the CANONICAL frontmatter
  format: title / created / updated / pinned / tags as a leading `---` block,
  then the Markdown body. The TipTap editor only ever touches the BODY; metadata
  is edited through the note UI. Rust does a lenient read-only parse of its own
  just to build the list — this file is the source of truth for writing.

  Saving is debounced (autosave): the editor pushes body/meta changes and we
  flush to disk a beat later, then patch the in-memory list entry in place.
*/
import { invoke } from '@tauri-apps/api/core';
import { writable, get, derived } from 'svelte/store';
import { marked } from 'marked';
import { looksLikeEscapedHtml, stripOuterMarkdownFence, sanitizeImportedMarkdown } from './notesMarkdown';
// Re-export the pure Markdown helpers so callers keep importing from '$lib/stores/notes'.
export { looksLikeEscapedHtml, stripOuterMarkdownFence, sanitizeImportedMarkdown };

/** List-row summary — matches the Rust `NoteSummary` (camelCase). */
export interface NoteSummary {
    path: string;
    title: string;
    preview: string;
    modifiedMs: number;
    pinned: boolean;
    tags: string[];
    /** Relative folder under the notes dir ("" = root, "Work", "Work/Specs"). */
    folder: string;
}

export interface NoteMeta {
    title: string;
    created: number;
    updated: number;
    pinned: boolean;
    tags: string[];
    /** Raw non-core YAML lines carried through saves until Properties owns them. */
    extraFrontmatter?: string;
}

/** A fully-loaded note: metadata + the Markdown body the editor edits. */
export interface NoteDoc {
    path: string;
    meta: NoteMeta;
    body: string;
}

/** Raw note content and its BLAKE3 revision, returned by `read_note`. */
export interface NoteFile {
    content: string;
    revision: string;
}

/** Result of a revision-checked `write_note` call. */
export interface NoteWriteResult {
    saved: boolean;
    revision: string | null;
    modifiedMs: number | null;
    conflict: 'changed' | 'missing' | null;
}

export interface ExternalNoteConflict {
    path: string;
    kind: 'changed' | 'missing';
    revision: string | null;
}

export const notes = writable<NoteSummary[]>([]);
export const notesLoading = writable(false);
export const notesReady = writable(false);
/** Conventional capture folder. Its files stay ordinary, portable `.ki` notes. */
export const INBOX_FOLDER = 'Inbox';
/** Visible, conventional home for one daily note per calendar day. */
export const DAILY_FOLDER = 'Daily';
export function isInboxFolder(folder: string | null | undefined): boolean {
    return folder?.toLowerCase() === INBOX_FOLDER.toLowerCase();
}

/** Local calendar key, intentionally independent of UTC so Today rolls over
 *  when the user's day does. */
export function localDateKey(date = new Date()): string {
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, '0');
    const day = String(date.getDate()).padStart(2, '0');
    return `${year}-${month}-${day}`;
}
/** Folder filter: `null` = all notes, `''` = root only, `'Work'` = that subfolder. */
export const currentFolder = writable<string | null>(null);
/** Every user subfolder (relative, forward-slash separated), kept in sync with the list. */
export const noteFolders = writable<string[]>([]);
/** The user's note templates (`.ki` stems in the `.templates/` dotfolder).
 *  Empty is the normal state — most users will never make one. */
export const noteTemplates = writable<string[]>([]);

export async function refreshNoteTemplates(): Promise<void> {
    try {
        noteTemplates.set(await invoke<string[]>('list_note_templates'));
    } catch {
        // No templates folder yet is not an error worth surfacing.
        noteTemplates.set([]);
    }
}

/** Create a note pre-filled from a template, with `{{date}}`-style snippet
 *  variables already resolved by the backend. Falls back to a blank note if
 *  the template can't be read — losing the template beats losing the click. */
export async function newNoteFromTemplate(name: string): Promise<void> {
    let body = '';
    try {
        body = await invoke<string>('read_note_template', { name });
    } catch (error) {
        console.warn('read_note_template failed:', error);
    }
    await newNote(name, body);
}

export async function openNoteTemplatesFolder(): Promise<void> {
    await invoke('open_note_templates_folder');
}

/** Every tag in use across all notes, de-duped and sorted — the tag input's
 *  autocomplete source. Purely derived: `notes` already carries each note's
 *  live `tags[]`, kept in sync on every save by `patchSummary`, so this needs
 *  no extra backend call and can never drift from the list. */
export const allTags = derived(notes, ($notes) => {
    const seen = new Set<string>();
    for (const n of $notes) for (const t of n.tags) if (t) seen.add(t);
    return [...seen].sort((a, b) => a.localeCompare(b));
});
/** The currently-open note, or null when nothing is selected. */
export const activeNote = writable<NoteDoc | null>(null);
/** A save was stopped because another app changed or removed this file. */
export const externalNoteConflict = writable<ExternalNoteConflict | null>(null);

/** Autosave status for the UI: 'saving' while a write is pending/in-flight,
 *  'saved' after a successful write, 'conflict' when user action is required,
 *  and 'idle' when there's nothing to save. */
export const saveStatus = writable<'idle' | 'saving' | 'saved' | 'conflict'>('idle');

const SAVE_DEBOUNCE_MS = 600;
let saveTimer: ReturnType<typeof setTimeout> | null = null;
/** True when the active note has edits not yet written. Lets blur / window-close
 *  flushes (and note switches) skip a pointless write when nothing changed. */
let dirty = false;
/** Incremented for each local edit so a write never clears a newer draft. */
let editGeneration = 0;
/** The active write, shared by blur, navigation, and explicit-save callers. */
let saveInFlight: Promise<boolean> | null = null;
/** Revision received with the active note. It is never synthesized in JS. */
let activeRevision: string | null = null;
/** A deliberate Keep mine after an external deletion may recreate the note. */
let restoringMissingNote = false;
/** Latest requested note open. Older disk reads must not replace a newer selection. */
let openGeneration = 0;

// ─── Incremental content indexing (best-effort, search integration) ─────────
// Each note is upserted into the content-search index so it's findable the
// moment it's saved — no full rebuild. Coalesced on a LONGER debounce than the
// autosave, so a typing session yields one index write, not one per save. Every
// call is fire-and-forget: if the content index isn't built or the write races
// a rebuild, it silently no-ops and the note still saves fine.
const INDEX_DEBOUNCE_MS = 2500;
let indexTimer: ReturnType<typeof setTimeout> | null = null;
const pendingIndex = new Set<string>();

function scheduleIndex(path: string): void {
    pendingIndex.add(path);
    if (indexTimer) clearTimeout(indexTimer);
    indexTimer = setTimeout(() => {
        const upserts = [...pendingIndex];
        pendingIndex.clear();
        indexTimer = null;
        void invoke('update_notes_index', { upserts, deletes: [] }).catch(() => {});
    }, INDEX_DEBOUNCE_MS);
}
function deindex(path: string): void {
    updateIndexPaths([], [path]);
}

/** Apply a completed file move/restore/delete to both search indexes immediately.
 *  Structural operations have exact before/after paths, so they need no rebuild. */
function updateIndexPaths(upserts: Iterable<string>, deletes: Iterable<string>): void {
    const next = [...new Set(upserts)];
    const nextSet = new Set(next);
    const removed = [...new Set(deletes)].filter((path) => !nextSet.has(path));
    for (const path of removed) pendingIndex.delete(path);
    if (!next.length && !removed.length) return;
    void invoke('update_notes_index', { upserts: next, deletes: removed }).catch(() => {});
}

// ─── Frontmatter (canonical serialize / parse) ─────────────────────────────
function yamlQuote(s: string): string {
    return `"${s.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}
function yamlUnquote(s: string): string {
    const t = s.trim();
    if (t.length >= 2 && ((t[0] === '"' && t.at(-1) === '"') || (t[0] === "'" && t.at(-1) === "'"))) {
        return t.slice(1, -1).replace(/\\"/g, '"').replace(/\\\\/g, '\\');
    }
    return t;
}

function parseTimestamp(value: string | undefined): number {
    const raw = yamlUnquote(value ?? '');
    const numeric = Number(raw);
    if (Number.isFinite(numeric)) return numeric;
    const parsed = Date.parse(raw);
    return Number.isFinite(parsed) ? parsed : 0;
}

/** Serialize meta + body into a `.ki` file string. */
export function serializeNote(meta: NoteMeta, body: string): string {
    const tags = meta.tags.length ? `[${meta.tags.map(yamlQuote).join(', ')}]` : '[]';
    const extra = meta.extraFrontmatter?.replace(/\r\n/g, '\n');
    const front = [
        '---',
        `title: ${yamlQuote(meta.title)}`,
        `created: ${meta.created}`,
        `updated: ${meta.updated}`,
        `pinned: ${meta.pinned ? 'true' : 'false'}`,
        `tags: ${tags}`,
        ...(extra ? [extra] : []),
        '---',
        '',
    ].join('\n');
    return `${front}\n${body.replace(/^\n+/, '')}`;
}

/** Parse a `.ki` file string into meta + body. Lenient: missing/!frontmatter
 *  files still load (title falls back to the first heading, then "Untitled"). */
export function parseNote(content: string, fallbackTitle = 'Untitled'): { meta: NoteMeta; body: string } {
    const text = content.replace(/\r\n/g, '\n');
    let body = text;
    const meta: NoteMeta = {
        title: '',
        created: 0,
        updated: 0,
        pinned: false,
        tags: [],
    };

    if (text.startsWith('---\n')) {
        const end = text.indexOf('\n---', 4);
        if (end !== -1) {
            const yaml = text.slice(4, end);
            const extraFrontmatter: string[] = [];
            for (const line of yaml.split('\n')) {
                // Indented keys belong to user-owned nested YAML and remain raw.
                const match = /^([A-Za-z][A-Za-z0-9_-]*):(.*)$/.exec(line);
                const key = match?.[1];
                const val = match?.[2].trim() ?? '';
                if (key === 'title') meta.title = yamlUnquote(val);
                else if (key === 'created') meta.created = parseTimestamp(val);
                else if (key === 'updated') meta.updated = parseTimestamp(val);
                else if (key === 'pinned') meta.pinned = val === 'true';
                else if (key === 'tags') {
                    const inner = val.replace(/^\[/, '').replace(/\]$/, '');
                    meta.tags = inner
                        .split(',')
                        .map((t) => yamlUnquote(t.trim()))
                        .filter(Boolean);
                } else extraFrontmatter.push(line);
            }
            if (extraFrontmatter.length) meta.extraFrontmatter = extraFrontmatter.join('\n');
            body = text.slice(end + 4).replace(/^\n+/, '');
        }
    }

    if (!meta.title) {
        const heading = body.split('\n').find((l) => l.trim().startsWith('# '));
        meta.title = heading ? heading.replace(/^#\s+/, '').trim() : fallbackTitle;
    }
    return { meta, body };
}

// ─── Plain-text / entity helpers (preview + escaped-HTML healing) ────────────
/**
 * Decode HTML entities EXACTLY ONCE (`&lt;` → `<`, `&amp;` → `&`, …). A
 * `<textarea>` decodes a single level only, which is what we want — a
 * double-encoded `&amp;lt;` becomes `&lt;`, not `<`. Browser-only (called
 * client-side from the editor/preview paths).
 */
export function decodeHtmlEntitiesOnce(s: string): string {
    if (!s || !/&(?:[a-z]+|#\d+|#x[0-9a-f]+);/i.test(s)) return s;
    const el = document.createElement('textarea');
    el.innerHTML = s;
    return el.value;
}

/** Clean, single-line plain text for sidebar previews. Heals escaped-HTML
 *  notes (decode once), renders Markdown/HTML to a DOM, and extracts text —
 *  so no `#`, `**`, raw tags, or `&lt;` entities ever reach the list. */
function cleanPreviewText(raw: string): string {
    let s = (raw ?? '').trim();
    if (!s) return '';
    if (looksLikeEscapedHtml(s) || /&(?:lt|gt|amp|quot|#0?39|#x27);/i.test(s)) {
        s = decodeHtmlEntitiesOnce(s);
    }
    s = stripOuterMarkdownFence(s);
    let text = s;
    try {
        const html = marked.parse(s) as string;
        const doc = new DOMParser().parseFromString(html, 'text/html');
        // Join top-level blocks with spaces so adjacent paragraphs don't merge.
        const blocks = Array.from(doc.body.children)
            .map((el) => (el.textContent ?? '').trim())
            .filter(Boolean);
        text = blocks.length ? blocks.join('  ') : doc.body.textContent ?? '';
    } catch {
        // Fall back to the (decoded) raw text if parsing is unavailable.
    }
    return text.replace(/\s+/g, ' ').trim().slice(0, 200);
}

// ─── Loading ────────────────────────────────────────────────────────────────
let initialized = false;
export async function initNotesStore(): Promise<void> {
    if (initialized) {
        void refreshNotes();
        return;
    }
    initialized = true;
    await refreshNotes();
    // One-time bulk upsert so pre-existing notes (and any dropped by a prior
    // content rebuild, which only scans configured roots) are findable in search
    // this session. Cheap for hundreds of small files; best-effort + no-op when
    // the content index isn't built.
    const paths = get(notes).map((n) => n.path);
    if (paths.length) {
        void invoke('update_notes_index', { upserts: paths, deletes: [] }).catch(() => {});
    }
}

export async function refreshNotes(): Promise<void> {
    notesLoading.set(true);
    try {
        const list = await invoke<NoteSummary[]>('list_notes');
        // Sanitize previews on the way in so escaped-HTML / Markdown never shows
        // in the sidebar, regardless of how the backend derived them.
        const rows = (Array.isArray(list) ? list : []).map((n) => ({
            ...n,
            preview: cleanPreviewText(n.preview),
        }));
        notes.set(rows);
        await refreshNoteFolders();
        notesReady.set(true);
    } catch (error) {
        console.warn('list_notes failed:', error);
    } finally {
        notesLoading.set(false);
    }
}

export async function openNote(path: string): Promise<void> {
    const request = ++openGeneration;
    // Flush any pending edits to the note we're leaving before switching.
    if (!(await flushSave())) return;
    try {
        const file = await invoke<NoteFile>('read_note', { path });
        if (request !== openGeneration) return;
        const { meta, body } = parseNote(file.content);
        activeNote.set({ path, meta, body });
        activeRevision = file.revision;
        restoringMissingNote = false;
        externalNoteConflict.set(null);
        editGeneration = 0;
        dirty = false; // freshly opened — nothing to save yet
        saveStatus.set('idle');
    } catch (error) {
        console.warn('read_note failed:', error);
    }
}

/** Discard the local draft and reload the external version after a conflict. */
export async function reloadActiveNote(): Promise<boolean> {
    const note = get(activeNote);
    if (!note) return false;
    const request = ++openGeneration;
    const reloadGeneration = editGeneration;
    try {
        const file = await invoke<NoteFile>('read_note', { path: note.path });
        if (request !== openGeneration || editGeneration !== reloadGeneration || get(activeNote)?.path !== note.path) {
            return false;
        }
        const { meta, body } = parseNote(file.content);
        activeNote.set({ path: note.path, meta, body });
        activeRevision = file.revision;
        restoringMissingNote = false;
        externalNoteConflict.set(null);
        editGeneration = 0;
        dirty = false;
        saveStatus.set('idle');
        await refreshNotes();
        return true;
    } catch (error) {
        console.warn('reloadActiveNote failed:', error);
        return false;
    }
}

/** Restore one local snapshot into the active note, preserving a newer draft if
 *  the user typed while the restore was in flight. */
export async function restoreActiveNoteRevision(id: string): Promise<boolean> {
    if (!id || !(await flushSave())) return false;
    const note = get(activeNote);
    if (!note || activeRevision === null) return false;

    const path = note.path;
    const restoreGeneration = editGeneration;
    try {
        const result = await invoke<NoteWriteResult>('restore_note_revision', {
            path,
            id,
            expectedRevision: activeRevision,
        });
        if (get(activeNote)?.path !== path) {
            await refreshNotes();
            updateIndexPaths([path], []);
            return result.saved;
        }
        if (!result.saved) {
            activeRevision = result.revision;
            restoringMissingNote = false;
            externalNoteConflict.set({
                path,
                kind: result.conflict === 'missing' ? 'missing' : 'changed',
                revision: result.revision,
            });
            saveStatus.set('conflict');
            return false;
        }
        if (editGeneration !== restoreGeneration) {
            activeRevision = result.revision;
            externalNoteConflict.set({ path, kind: 'changed', revision: result.revision });
            saveStatus.set('conflict');
            return false;
        }
        if (!(await reloadActiveNote())) return false;
        updateIndexPaths([path], []);
        return true;
    } catch (error) {
        console.warn('restore_note_revision failed:', error);
        return false;
    }
}

/** Retry the local draft against the revision shown in the conflict banner. */
export async function keepActiveNoteChanges(): Promise<boolean> {
    const note = get(activeNote);
    const conflict = get(externalNoteConflict);
    if (!note || !conflict || conflict.path !== note.path) return false;
    activeRevision = conflict.revision;
    restoringMissingNote = conflict.kind === 'missing';
    externalNoteConflict.set(null);
    dirty = true;
    return flushSave();
}

export async function closeActiveNote(): Promise<boolean> {
    if (!(await flushSave())) return false;
    activeNote.set(null);
    activeRevision = null;
    externalNoteConflict.set(null);
    editGeneration = 0;
    return true;
}

// ─── Mutations (each schedules an autosave) ─────────────────────────────────
export function setActiveBody(body: string): void {
    activeNote.update((n) => (n ? { ...n, body } : n));
    markActiveNoteDirty();
}
export function setActiveTitle(title: string): void {
    activeNote.update((n) => (n ? { ...n, meta: { ...n.meta, title } } : n));
    markActiveNoteDirty();
}
export function toggleActivePin(): void {
    activeNote.update((n) => (n ? { ...n, meta: { ...n.meta, pinned: !n.meta.pinned } } : n));
    markActiveNoteDirty();
    void flushSave(); // pin is a deliberate action — persist immediately
}
/** Replace the active note's tags. Always pass a NEW array — callers must not
 *  mutate in place, or Svelte won't see the change.
 *
 *  Persists immediately rather than debounced: adding/removing a tag is a
 *  discrete committed action like pinning, not a keystroke stream like the
 *  title or body. Other frontmatter can't be clobbered here — `flushSave`
 *  re-serializes the whole meta object, so title/created/pinned come along
 *  untouched. */
export function setActiveTags(tags: string[]): void {
    activeNote.update((n) => (n ? { ...n, meta: { ...n.meta, tags } } : n));
    markActiveNoteDirty();
    void flushSave();
}

function markActiveNoteDirty(): void {
    dirty = true;
    editGeneration += 1;
    const note = get(activeNote);
    if (note && get(externalNoteConflict)?.path === note.path) {
        saveStatus.set('conflict');
        return;
    }
    saveStatus.set('saving');
    scheduleSave();
}

function scheduleSave(): void {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => void flushSave(), SAVE_DEBOUNCE_MS);
}

/** Write the active note to disk now (if any) and patch its list row.
 *  Returns false when an attempted write fails, so callers that are about to
 *  rename or remove the file can leave the note in place for a retry. */
async function writeActiveNote(): Promise<boolean> {
    if (saveTimer) {
        clearTimeout(saveTimer);
        saveTimer = null;
    }
    const note = get(activeNote);
    const conflict = get(externalNoteConflict);
    if (dirty && conflict?.path === note?.path) {
        saveStatus.set('conflict');
        return false;
    }
    if (dirty && note && activeRevision === null && !restoringMissingNote) {
        console.warn('Cannot save active note without a disk revision');
        return false;
    }
    if (!note || !dirty) return true; // nothing unsaved → skip the write entirely
    const savingGeneration = editGeneration;
    saveStatus.set('saving');
    const meta: NoteMeta = { ...note.meta, updated: Date.now() };
    const content = serializeNote(meta, note.body);
    try {
        const result = await invoke<NoteWriteResult>('write_note', {
            path: note.path,
            content,
            expectedRevision: activeRevision,
        });
        if (!result.saved) {
            activeRevision = result.revision;
            restoringMissingNote = false;
            externalNoteConflict.set({
                path: note.path,
                kind: result.conflict === 'missing' ? 'missing' : 'changed',
                revision: result.revision,
            });
            saveStatus.set('conflict');
            return false;
        }
        activeRevision = result.revision;
        restoringMissingNote = false;
        if (editGeneration !== savingGeneration) {
            // A newer local edit landed while this write was in flight. Keep
            // it dirty and serialize a second write against the new revision.
            dirty = true;
            return writeActiveNote();
        }
        dirty = false;
        saveStatus.set('saved');
        activeNote.update((n) => (n && n.path === note.path ? { ...n, meta } : n));
        patchSummary(note.path, meta, note.body, result.modifiedMs ?? Date.now());
        scheduleIndex(note.path);
        return true;
    } catch (error) {
        // Leave `dirty` true so a later flush retries; surface the failure state.
        saveStatus.set('saving');
        console.warn('write_note failed:', error);
        return false;
    }
}

/**
 * Flush the active draft through one serialized write pipeline. Multiple
 * callers (autosave, blur, navigation) share the same promise, so an older
 * write cannot mark a newer local edit as saved.
 */
export async function flushSave(): Promise<boolean> {
    if (saveTimer) {
        clearTimeout(saveTimer);
        saveTimer = null;
    }
    if (saveInFlight) return saveInFlight;

    const pending = writeActiveNote();
    saveInFlight = pending;
    try {
        return await pending;
    } finally {
        if (saveInFlight === pending) saveInFlight = null;
    }
}

/** Update a list row in place after a save (avoids a full re-scan per keystroke). */
function patchSummary(path: string, meta: NoteMeta, body: string, modifiedMs: number): void {
    notes.update((list) => {
        const preview = cleanPreviewText(body);
        const next = list.map((n) =>
            n.path === path
                ? { ...n, title: meta.title, preview, pinned: meta.pinned, tags: meta.tags, modifiedMs }
                : n,
        );
        // Re-sort: pinned first, then most-recent.
        next.sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.modifiedMs - a.modifiedMs);
        return next;
    });
}

// ─── Create / delete ────────────────────────────────────────────────────────
export async function newNote(title = 'Untitled', body = ''): Promise<void> {
    if (!(await flushSave())) return;
    const now = Date.now();
    const meta: NoteMeta = { title, created: now, updated: now, pinned: false, tags: [] };
    try {
        let summary = await invoke<NoteSummary>('create_note', {
            fileBase: title,
            content: serializeNote(meta, body),
        });
        // If a specific folder is active, create then move. Keeping this
        // fallback preserves a root note if that folder disappeared mid-click.
        const target = get(currentFolder);
        if (target) {
            try {
                const newPath = await invoke<string>('move_note', {
                    path: summary.path,
                    targetFolder: target,
                });
                summary = { ...summary, path: newPath, folder: target };
            } catch (moveError) {
                console.warn('move_note (new note) failed:', moveError);
            }
        }
        await refreshNotes();
        await openNote(summary.path);
        scheduleIndex(summary.path);
    } catch (error) {
        console.warn('create_note failed:', error);
    }
}

/** Open the one portable note for a local calendar day. `Daily Note.ki`, when
 *  present in templates, supplies the body; no template still produces a
 *  useful blank daily note. */
export async function openTodayNote(
    dateKey: string,
    title: string,
    templateName?: string,
): Promise<NoteSummary | null> {
    if (!(await flushSave())) return null;

    let body = '';
    if (templateName) {
        try {
            body = await invoke<string>('read_note_template', { name: templateName });
        } catch (error) {
            console.warn('read daily note template failed:', error);
        }
    }

    const now = Date.now();
    const meta: NoteMeta = { title, created: now, updated: now, pinned: false, tags: [] };
    try {
        const summary = await invoke<NoteSummary>('open_or_create_daily_note', {
            dateKey,
            content: serializeNote(meta, body),
        });
        await refreshNotes();
        await openNote(summary.path);
        scheduleIndex(summary.path);
        return summary;
    } catch (error) {
        console.warn('open_or_create_daily_note failed:', error);
        return null;
    }
}

/** Quick-capture: create a note from raw text (palette `note:` quick-add,
 *  "Send to note" from Clipboard, etc.). Title = first line (capped); body =
 *  the full text. Does NOT open the note — it's a background capture — but it
 *  refreshes the list so an open Notes page reflects it. Returns the summary. */
export async function captureNote(text: string, preferredTitle?: string): Promise<NoteSummary | null> {
    const trimmed = text.trim();
    if (!trimmed) return null;
    const firstLine = trimmed.split('\n')[0].trim();
    const requested = preferredTitle?.trim();
    const title =
        (requested ? requested.slice(0, 120) : firstLine.length > 60 ? firstLine.slice(0, 60) : firstLine) ||
        'Quick note';
    const now = Date.now();
    const meta: NoteMeta = { title, created: now, updated: now, pinned: false, tags: [] };
    try {
        const summary = await invoke<NoteSummary>('create_note', {
            fileBase: title,
            content: serializeNote(meta, trimmed),
            targetFolder: INBOX_FOLDER,
        });
        await refreshNotes();
        scheduleIndex(summary.path);
        return summary;
    } catch (error) {
        console.warn('captureNote failed:', error);
        return null;
    }
}

export async function removeNote(path: string): Promise<void> {
    if (get(activeNote)?.path === path && !(await flushSave())) return;
    try {
        await invoke('delete_note', { path });
        deindex(path);
        if (get(activeNote)?.path === path) {
            if (saveTimer) {
                clearTimeout(saveTimer);
                saveTimer = null;
            }
            activeNote.set(null);
            activeRevision = null;
            externalNoteConflict.set(null);
        }
        await refreshNotes();
        // Open the next note so the editor pane isn't left blank if any remain.
        const remaining = get(notes);
        if (!get(activeNote) && remaining.length) {
            await openNote(remaining[0].path);
        }
    } catch (error) {
        console.warn('delete_note failed:', error);
    }
}

export async function togglePin(path: string): Promise<void> {
    const active = get(activeNote);
    if (active && active.path === path) {
        toggleActivePin();
        return;
    }
    // Non-active note: read, flip, write back.
    try {
        const file = await invoke<NoteFile>('read_note', { path });
        const { meta, body } = parseNote(file.content);
        meta.pinned = !meta.pinned;
        meta.updated = Date.now();
        const result = await invoke<NoteWriteResult>('write_note', {
            path,
            content: serializeNote(meta, body),
            expectedRevision: file.revision,
        });
        if (!result.saved) {
            await refreshNotes();
            return;
        }
        patchSummary(path, meta, body, result.modifiedMs ?? Date.now());
        scheduleIndex(path);
    } catch (error) {
        console.warn('togglePin failed:', error);
    }
}

// ─── Note subfolders ────────────────────────────────────────────────────────
export async function refreshNoteFolders(): Promise<void> {
    try {
        const list = await invoke<string[]>('list_note_folders');
        noteFolders.set(Array.isArray(list) ? list : []);
    } catch (error) {
        console.warn('list_note_folders failed:', error);
    }
}

/** Create a (possibly nested, forward-slash) subfolder. Throws on failure so the
 *  caller can surface it. */
export async function createNoteFolder(folder: string): Promise<void> {
    const name = folder.trim();
    if (!name) return;
    await invoke('create_note_folder', { folder: name });
    await refreshNoteFolders();
}

export async function renameNoteFolder(from: string, to: string): Promise<void> {
    const target = to.trim();
    if (!from || !target || from === target) return;
    const oldPaths = notePathsInFolder(from);
    await invoke('rename_note_folder', { from, to: target });
    const selected = get(currentFolder);
    if (isFolderOrDescendant(selected, from)) {
        currentFolder.set(`${target}${selected.slice(from.length)}`);
    }
    await refreshNotes();
    updateIndexPaths(notePathsInFolder(target), oldPaths);
}

/** A recoverable note or folder sitting in `.trash`. */
export interface TrashedNote {
    path: string;
    title: string;
    deletedMs: number;
    kind: 'note' | 'folder';
}

/** Contents of the trash, newest first. The trash was write-only until
 *  2026-07-16 — notes went in and nothing could list or recover them. */
export async function listTrashedNotes(): Promise<TrashedNote[]> {
    try {
        return await invoke<TrashedNote[]>('list_trashed_notes');
    } catch (error) {
        console.warn('list_trashed_notes failed:', error);
        return [];
    }
}

/** Restore a trashed note or folder, then refresh every restored note in search. */
export async function restoreTrashedNote(path: string): Promise<void> {
    const restoredPaths = await invoke<string[]>('restore_trashed_note', { path });
    await refreshNotes();
    updateIndexPaths(restoredPaths, []);
}

/** Permanently delete ONE trashed note. Irreversible. */
export async function deleteTrashedNote(path: string): Promise<void> {
    await invoke('delete_trashed_note', { path });
}

/** Permanently delete everything in the trash. Returns how many went. */
export async function emptyNoteTrash(): Promise<number> {
    return await invoke<number>('empty_note_trash');
}

/** Move a folder (and its notes) to the trash — reversible, like deleting a note. */
export async function deleteNoteFolder(folder: string): Promise<void> {
    if (!folder) return;
    const deletedPaths = notePathsInFolder(folder);
    await invoke('delete_note_folder', { folder });
    if (isFolderOrDescendant(get(currentFolder), folder)) currentFolder.set(null);
    await refreshNotes();
    updateIndexPaths([], deletedPaths);
}

/** Move a note into `targetFolder` (empty string = root). Returns its new path. */
export async function moveNote(path: string, targetFolder: string): Promise<string> {
    const newPath = await invoke<string>('move_note', { path, targetFolder });
    await refreshNotes();
    updateIndexPaths([newPath], [path]);
    return newPath;
}

function isFolderOrDescendant(folder: string | null, parent: string): folder is string {
    return folder === parent || Boolean(folder?.startsWith(`${parent}/`));
}

function notePathsInFolder(folder: string): string[] {
    return get(notes)
        .filter((note) => isFolderOrDescendant(note.folder, folder))
        .map((note) => note.path);
}

// ─── Folder helpers ─────────────────────────────────────────────────────────
export async function getNotesDir(): Promise<string> {
    try {
        return await invoke<string>('get_notes_dir');
    } catch {
        return '';
    }
}
export async function openNotesFolder(): Promise<void> {
    try {
        await invoke('open_notes_folder');
    } catch (error) {
        console.warn('open_notes_folder failed:', error);
    }
}
