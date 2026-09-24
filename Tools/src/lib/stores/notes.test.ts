import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest';
import { get } from 'svelte/store';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

import {
    INBOX_FOLDER,
    activeNote,
    captureNote,
    currentFolder,
    deleteNoteFolder,
    externalNoteConflict,
    flushSave,
    isInboxFolder,
    keepActiveNoteChanges,
    localDateKey,
    moveNote,
    notes,
    openTodayNote,
    openNote,
    parseNote,
    removeNote,
    renameNoteFolder,
    restoreActiveNoteRevision,
    restoreTrashedNote,
    saveStatus,
    serializeNote,
    setActiveBody,
    type NoteMeta,
} from './notes';

/* The tag-assignment UI leans entirely on one property: tags survive a
 * save/load round trip without disturbing the rest of the frontmatter. That
 * was never exercised before, because nothing could assign a tag. */
describe('note frontmatter tag round-trip', () => {
    const base: NoteMeta = {
        title: 'Quarterly review',
        created: 1_700_000_000_000,
        updated: 1_700_000_500_000,
        pinned: true,
        tags: [],
    };

    it('round-trips tags without corrupting other frontmatter', () => {
        // 'Q3 2026' has a space, 'Café' is non-ASCII — both have historically
        // been where naive YAML writers break.
        const meta: NoteMeta = { ...base, tags: ['work', 'Q3 2026', 'Café'] };

        const { meta: out, body } = parseNote(serializeNote(meta, 'body text'));

        expect(out.tags).toEqual(['work', 'Q3 2026', 'Café']);
        // The whole point of persisting via a full re-serialize: a tag edit
        // must not clobber the fields it isn't touching.
        expect(out.title).toBe('Quarterly review');
        expect(out.pinned).toBe(true);
        expect(out.created).toBe(1_700_000_000_000);
        expect(body.trim()).toBe('body text');
    });

    it('round-trips an empty tag list', () => {
        const { meta: out } = parseNote(serializeNote(base, 'body'));
        expect(out.tags).toEqual([]);
        expect(out.title).toBe('Quarterly review');
    });

    it('preserves user-owned YAML while core fields are updated', () => {
        const raw = [
            '---',
            'title: Original title',
            'created: 1700000000000',
            'updated: 1700000001000',
            'pinned: false',
            'tags: [work]',
            '# Keep this comment',
            'project: KeepItLocal',
            'details:',
            '  owner: neo',
            '  title: nested title',
            '---',
            '',
            'Body stays here.',
        ].join('\n');

        const { meta, body } = parseNote(raw);
        const saved = serializeNote({ ...meta, title: 'Updated title', tags: ['notes'] }, body);

        expect(saved).toContain('# Keep this comment');
        expect(saved).toContain('project: KeepItLocal');
        expect(saved).toContain('details:\n  owner: neo\n  title: nested title');
        expect(saved).toContain('title: "Updated title"');
        expect(saved).toContain('tags: ["notes"]');
        expect(saved).toContain('Body stays here.');
    });

    it('reads legacy ISO timestamps so a later save does not reset them', () => {
        const { meta } = parseNote(
            '---\ncreated: 2026-07-30T12:00:00.000Z\nupdated: 2026-07-30T13:00:00.000Z\n---\nbody',
        );

        expect(meta.created).toBe(Date.parse('2026-07-30T12:00:00.000Z'));
        expect(meta.updated).toBe(Date.parse('2026-07-30T13:00:00.000Z'));
    });

    /* The frontmatter tag list is split on ',' in three separate places
     * (here, notes.rs parse_tags, preview.ts) with a naive split that does
     * not honour quoting. A tag containing a comma would therefore tear into
     * two tags on reload. The chip input makes ',' a commit key so one can't
     * be created — this records WHY that constraint exists, so nobody
     * "helpfully" removes it later.
     *
     * If a future change makes the parsers quote-aware, this expectation is
     * the thing to revisit — flip it to expect the comma to survive. */
    it('documents the known comma limitation the UI guards against', () => {
        const meta: NoteMeta = { ...base, tags: ['a,b'] };
        const { meta: out } = parseNote(serializeNote(meta, 'body'));
        expect(out.tags).not.toEqual(['a,b']);
    });
});

describe('Inbox folder convention', () => {
    it('matches the top-level conventional Inbox folder regardless of Windows casing', () => {
        expect(isInboxFolder(INBOX_FOLDER)).toBe(true);
        expect(isInboxFolder('Work/Inbox')).toBe(false);
        expect(isInboxFolder('inbox')).toBe(true);
    });

    it('uses the local calendar day for a daily-note key', () => {
        expect(localDateKey(new Date(2026, 6, 3, 0, 5))).toBe('2026-07-03');
    });

    it('keeps an explicit import title while saving the capture to Inbox', async () => {
        vi.useFakeTimers();
        invokeMock.mockReset();
        let createArgs: Record<string, string> | undefined;
        const summary = {
            path: 'Inbox/imported-brief.ki',
            title: 'Imported brief',
            preview: 'Body from the imported file',
            modifiedMs: 1,
            pinned: false,
            tags: [],
            folder: INBOX_FOLDER,
        };
        invokeMock.mockImplementation(async (command: string, args?: Record<string, string>) => {
            if (command === 'create_note') {
                createArgs = args;
                return summary;
            }
            if (command === 'list_notes') return [summary];
            if (command === 'list_note_folders') return [INBOX_FOLDER];
            return undefined;
        });

        const saved = await captureNote('Body from the imported file', 'Imported brief');

        expect(saved?.folder).toBe(INBOX_FOLDER);
        expect(createArgs?.targetFolder).toBe(INBOX_FOLDER);
        expect(parseNote(createArgs?.content ?? '').meta.title).toBe('Imported brief');
        vi.clearAllTimers();
        vi.useRealTimers();
    });
});

describe('note selection', () => {
    beforeEach(() => {
        invokeMock.mockReset();
        activeNote.set(null);
        externalNoteConflict.set(null);
    });

    afterEach(() => {
        activeNote.set(null);
        externalNoteConflict.set(null);
    });

    it('keeps the last note selected when an earlier disk read finishes late', async () => {
        const first = serializeNote({ title: 'First', created: 1, updated: 1, pinned: false, tags: [] }, 'First body');
        const second = serializeNote({ title: 'Second', created: 2, updated: 2, pinned: false, tags: [] }, 'Second body');
        let resolveFirst: ((file: { content: string; revision: string }) => void) | undefined;
        let resolveSecond: ((file: { content: string; revision: string }) => void) | undefined;

        invokeMock.mockImplementation((command: string, args?: { path?: string }) => {
            if (command !== 'read_note') return undefined;
            return new Promise((resolve) => {
                if (args?.path === 'first.ki') resolveFirst = resolve;
                if (args?.path === 'second.ki') resolveSecond = resolve;
            });
        });

        const openingFirst = openNote('first.ki');
        const openingSecond = openNote('second.ki');
        for (let i = 0; i < 4; i += 1) await Promise.resolve();

        expect(resolveFirst).toBeTypeOf('function');
        expect(resolveSecond).toBeTypeOf('function');
        resolveSecond?.({ content: second, revision: 'second-revision' });
        await openingSecond;
        resolveFirst?.({ content: first, revision: 'first-revision' });
        await openingFirst;

        expect(get(activeNote)).toMatchObject({ path: 'second.ki', body: 'Second body' });
    });
});

describe('structural note changes keep search indexed', () => {
    const row = (path: string, folder: string) => ({
        path,
        folder,
        title: path,
        preview: '',
        modifiedMs: 1,
        pinned: false,
        tags: [],
    });

    beforeEach(() => {
        invokeMock.mockReset();
        notes.set([]);
        currentFolder.set(null);
        activeNote.set(null);
    });

    afterEach(() => {
        notes.set([]);
        currentFolder.set(null);
        activeNote.set(null);
    });

    it('replaces an indexed note path after a move', async () => {
        const oldPath = 'Work/brief.ki';
        const newPath = 'Archive/brief.ki';
        let rows = [row(oldPath, 'Work')];
        invokeMock.mockImplementation(async (command: string) => {
            if (command === 'move_note') {
                rows = [row(newPath, 'Archive')];
                return newPath;
            }
            if (command === 'list_notes') return rows;
            if (command === 'list_note_folders') return ['Archive'];
            return undefined;
        });

        await expect(moveNote(oldPath, 'Archive')).resolves.toBe(newPath);
        expect(invokeMock).toHaveBeenCalledWith('update_notes_index', {
            upserts: [newPath],
            deletes: [oldPath],
        });
    });

    it('replaces every descendant path after a folder rename and follows the selected descendant', async () => {
        const oldPaths = ['Work/brief.ki', 'Work/Sub/spec.ki'];
        const newPaths = ['Archive/brief.ki', 'Archive/Sub/spec.ki'];
        notes.set([row(oldPaths[0], 'Work'), row(oldPaths[1], 'Work/Sub'), row('Other.ki', '')]);
        currentFolder.set('Work/Sub');
        let rows = [row(newPaths[0], 'Archive'), row(newPaths[1], 'Archive/Sub'), row('Other.ki', '')];
        invokeMock.mockImplementation(async (command: string) => {
            if (command === 'list_notes') return rows;
            if (command === 'list_note_folders') return ['Archive', 'Archive/Sub'];
            return undefined;
        });

        await renameNoteFolder('Work', 'Archive');

        expect(get(currentFolder)).toBe('Archive/Sub');
        expect(invokeMock).toHaveBeenCalledWith('update_notes_index', {
            upserts: newPaths,
            deletes: oldPaths,
        });
    });

    it('drops every descendant path after deleting a folder and clears a selected descendant', async () => {
        const removedPaths = ['Work/brief.ki', 'Work/Sub/spec.ki'];
        notes.set([row(removedPaths[0], 'Work'), row(removedPaths[1], 'Work/Sub'), row('Other.ki', '')]);
        currentFolder.set('Work/Sub');
        invokeMock.mockImplementation(async (command: string) => {
            if (command === 'list_notes') return [row('Other.ki', '')];
            if (command === 'list_note_folders') return [];
            return undefined;
        });

        await deleteNoteFolder('Work');

        expect(get(currentFolder)).toBeNull();
        expect(invokeMock).toHaveBeenCalledWith('update_notes_index', {
            upserts: [],
            deletes: removedPaths,
        });
    });

    it('adds every note restored from trash and drops a deleted note from the index', async () => {
        const restoredPaths = ['restored.ki', 'Restored folder/nested.ki'];
        const deletedPath = 'obsolete.ki';
        let rows = [row(deletedPath, '')];
        invokeMock.mockImplementation(async (command: string) => {
            if (command === 'restore_trashed_note') {
                rows = [row(restoredPaths[0], ''), row(restoredPaths[1], 'Restored folder'), ...rows];
                return restoredPaths;
            }
            if (command === 'delete_note') {
                rows = rows.filter((note) => note.path !== deletedPath);
                return undefined;
            }
            if (command === 'read_note') return { content: serializeNote({ title: 'Restored', created: 1, updated: 1, pinned: false, tags: [] }, ''), revision: 'restored-revision' };
            if (command === 'list_notes') return rows;
            if (command === 'list_note_folders') return [];
            return undefined;
        });

        await restoreTrashedNote('.trash/restored.ki');
        await removeNote(deletedPath);

        expect(invokeMock).toHaveBeenCalledWith('update_notes_index', {
            upserts: restoredPaths,
            deletes: [],
        });
        expect(invokeMock).toHaveBeenCalledWith('update_notes_index', {
            upserts: [],
            deletes: [deletedPath],
        });
    });
});

describe('revision-checked note saves', () => {
    beforeEach(() => {
        vi.useFakeTimers();
        invokeMock.mockReset();
        activeNote.set(null);
        externalNoteConflict.set(null);
        saveStatus.set('idle');
    });

    afterEach(() => {
        vi.clearAllTimers();
        vi.useRealTimers();
        activeNote.set(null);
        externalNoteConflict.set(null);
    });

    it('keeps an external version intact until Keep mine is chosen', async () => {
        const meta: NoteMeta = {
            title: 'Shared note',
            created: 1,
            updated: 1,
            pinned: false,
            tags: [],
        };
        const opened = serializeNote(meta, 'Original body');
        const external = serializeNote({ ...meta, updated: 2 }, 'External body');
        let disk = opened;
        const revision = (content: string) =>
            content === opened ? 'opened-revision' : content === external ? 'external-revision' : 'saved-revision';

        invokeMock.mockImplementation(async (command: string, args?: { expectedRevision?: string | null; content?: string }) => {
            if (command === 'read_note') return { content: disk, revision: revision(disk) };
            if (command === 'write_note') {
                if (args?.expectedRevision !== revision(disk)) {
                    return { saved: false, revision: revision(disk), modifiedMs: null, conflict: 'changed' };
                }
                disk = args?.content ?? '';
                return { saved: true, revision: revision(disk), modifiedMs: 3, conflict: null };
            }
            return undefined;
        });

        await openNote('shared.ki');
        disk = external;
        setActiveBody('My local body');

        await expect(flushSave()).resolves.toBe(false);
        expect(disk).toBe(external);
        expect(get(activeNote)?.body).toBe('My local body');
        expect(get(externalNoteConflict)).toEqual({
            path: 'shared.ki',
            kind: 'changed',
            revision: 'external-revision',
        });

        await expect(keepActiveNoteChanges()).resolves.toBe(true);
        expect(disk).toContain('My local body');
        expect(get(externalNoteConflict)).toBeNull();
    });

    it('saves a newer edit made while the first write is still in flight', async () => {
        const meta: NoteMeta = {
            title: 'Shared note',
            created: 1,
            updated: 1,
            pinned: false,
            tags: [],
        };
        let disk = serializeNote(meta, 'Original body');
        let revision = 'opened-revision';
        const writes: string[] = [];
        let finishFirstWrite: (() => void) | undefined;

        invokeMock.mockImplementation(async (command: string, args?: { expectedRevision?: string | null; content?: string }) => {
            if (command === 'read_note') return { content: disk, revision };
            if (command === 'write_note') {
                if (args?.expectedRevision !== revision) {
                    return { saved: false, revision, modifiedMs: null, conflict: 'changed' };
                }
                const content = args?.content ?? '';
                writes.push(content);
                if (writes.length === 1) {
                    return new Promise((resolve) => {
                        finishFirstWrite = () => {
                            disk = content;
                            revision = 'first-write-revision';
                            resolve({ saved: true, revision, modifiedMs: 2, conflict: null });
                        };
                    });
                }
                disk = content;
                revision = 'second-write-revision';
                return { saved: true, revision, modifiedMs: 3, conflict: null };
            }
            return undefined;
        });

        await openNote('shared.ki');
        setActiveBody('First local body');
        const firstFlush = flushSave();
        setActiveBody('Newest local body');
        finishFirstWrite?.();

        await expect(firstFlush).resolves.toBe(true);
        expect(writes).toHaveLength(2);
        expect(writes[0]).toContain('First local body');
        expect(writes[1]).toContain('Newest local body');
        expect(disk).toContain('Newest local body');
    });

    it('only restores a missing note after an explicit Keep mine retry', async () => {
        const meta: NoteMeta = {
            title: 'Shared note',
            created: 1,
            updated: 1,
            pinned: false,
            tags: [],
        };
        const opened = serializeNote(meta, 'Original body');
        let disk: string | null = opened;

        invokeMock.mockImplementation(async (command: string, args?: { expectedRevision?: string | null; content?: string }) => {
            if (command === 'read_note') {
                if (disk === null) throw new Error('missing');
                return { content: disk, revision: 'opened-revision' };
            }
            if (command === 'write_note') {
                if (disk === null && args?.expectedRevision !== null) {
                    return { saved: false, revision: null, modifiedMs: null, conflict: 'missing' };
                }
                disk = args?.content ?? '';
                return { saved: true, revision: 'restored-revision', modifiedMs: 3, conflict: null };
            }
            return undefined;
        });

        await openNote('shared.ki');
        disk = null;
        setActiveBody('My local body');

        await expect(flushSave()).resolves.toBe(false);
        expect(disk).toBeNull();
        expect(get(externalNoteConflict)).toEqual({
            path: 'shared.ki',
            kind: 'missing',
            revision: null,
        });

        await expect(keepActiveNoteChanges()).resolves.toBe(true);
        expect(disk).toContain('My local body');
    });

    it('restores a snapshot into the active note and reindexes the restored file', async () => {
        const meta: NoteMeta = { title: 'Shared note', created: 1, updated: 1, pinned: false, tags: [] };
        const original = serializeNote(meta, 'Original body');
        const restored = serializeNote({ ...meta, updated: 2 }, 'Restored body');
        let disk = original;
        const summary = {
            path: 'shared.ki',
            title: 'Shared note',
            preview: 'Restored body',
            modifiedMs: 2,
            pinned: false,
            tags: [],
            folder: '',
        };

        invokeMock.mockImplementation(async (command: string, args?: { id?: string; expectedRevision?: string | null }) => {
            if (command === 'read_note') return { content: disk, revision: disk === original ? 'opened-revision' : 'restored-revision' };
            if (command === 'restore_note_revision') {
                expect(args).toEqual({ path: 'shared.ki', id: 'snapshot-1', expectedRevision: 'opened-revision' });
                disk = restored;
                return { saved: true, revision: 'restored-revision', modifiedMs: 2, conflict: null };
            }
            if (command === 'list_notes') return [summary];
            if (command === 'list_note_folders') return [];
            return undefined;
        });

        await openNote(summary.path);

        await expect(restoreActiveNoteRevision('snapshot-1')).resolves.toBe(true);
        expect(get(activeNote)?.body).toBe('Restored body');
        expect(invokeMock).toHaveBeenCalledWith('update_notes_index', {
            upserts: [summary.path],
            deletes: [],
        });
    });
});

describe('daily note flow', () => {
    beforeEach(() => {
        vi.useFakeTimers();
        invokeMock.mockReset();
        activeNote.set(null);
        externalNoteConflict.set(null);
    });

    afterEach(() => {
        vi.clearAllTimers();
        vi.useRealTimers();
        activeNote.set(null);
        externalNoteConflict.set(null);
    });

    it('uses the conventional Daily Note template when creating today', async () => {
        const summary = {
            path: 'Daily/2026-07-30.ki',
            title: 'Wednesday, July 30, 2026',
            preview: 'Template body',
            modifiedMs: 1,
            pinned: false,
            tags: [],
            folder: 'Daily',
        };
        let createdContent = '';
        invokeMock.mockImplementation(async (command: string, args?: Record<string, string>) => {
            if (command === 'read_note_template') return 'Template body';
            if (command === 'open_or_create_daily_note') {
                createdContent = args?.content ?? '';
                return summary;
            }
            if (command === 'list_notes') return [summary];
            if (command === 'list_note_folders') return ['Daily'];
            if (command === 'read_note') return { content: createdContent, revision: 'daily-revision' };
            return undefined;
        });

        const result = await openTodayNote('2026-07-30', summary.title, 'Daily Note');

        expect(result?.path).toBe(summary.path);
        expect(createdContent).toContain('title: "Wednesday, July 30, 2026"');
        expect(createdContent).toContain('Template body');
        expect(get(activeNote)?.path).toBe(summary.path);
    });
});
