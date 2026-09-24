/*
  Tests for parseNoteMarkdown — the pure parse path with no DOM
  dependency. The sanitizer + markdown render path live in
  `sanitizeNoteHtml` / `renderNoteHtml` and need a DOMParser to exercise,
  which this project's vitest config doesn't currently provide (no jsdom
  / happy-dom). Adding that for a single helper would be more weight
  than it's worth — those functions are exercised at runtime by the
  command-palette preview, and any change to the sanitizer allowlists
  should be reviewed visually against the live preview anyway.

  If we later add jsdom (e.g. for component tests), the sanitizer cases
  from the old draft of this file are still in git history — restore
  them in one diff.
*/
import { describe, expect, it } from 'vitest';

import { isSafeUrl, parseNoteMarkdown } from './preview';

describe('preview URL policy', () => {
    it('allows explicit web/mail links and relative paths only', () => {
        expect(isSafeUrl('https://keepitlocal.app/notes')).toBe(true);
        expect(isSafeUrl('http://localhost:5173/notes')).toBe(true);
        expect(isSafeUrl('mailto:hello@example.com')).toBe(true);
        expect(isSafeUrl('../attachments/report.pdf')).toBe(true);
        expect(isSafeUrl('#heading')).toBe(true);

        expect(isSafeUrl('//example.com')).toBe(false);
        expect(isSafeUrl('file:///C:/notes/private.pdf')).toBe(false);
        expect(isSafeUrl('javascript:alert(1)')).toBe(false);
        expect(isSafeUrl('data:text/html,unsafe')).toBe(false);
        expect(isSafeUrl('ftp://example.com/archive')).toBe(false);
    });
});

describe('parseNoteMarkdown', () => {
    it('extracts frontmatter fields and strips the block from body', () => {
        const raw = [
            '---',
            'title: "hello"',
            'created: 0',
            'updated: 1779763236123',
            'pinned: false',
            'tags: []',
            '---',
            '',
            '# KeepItLocal',
            '',
            'Body.',
        ].join('\n');
        const note = parseNoteMarkdown(raw, 'hello.ki');
        expect(note.title).toBe('hello');
        expect(note.createdAt).toBeNull(); // 0 maps to null, not 1970
        expect(note.updatedAt).toBe(1779763236123);
        expect(note.pinned).toBe(false);
        expect(note.tags).toEqual([]);
        expect(note.frontmatterValid).toBe(true);
        expect(note.bodyMarkdown.startsWith('# KeepItLocal')).toBe(true);
        // Frontmatter delimiters must NOT leak into body.
        expect(note.bodyMarkdown.includes('---')).toBe(false);
    });

    it('parses tags array with mixed quoting', () => {
        const raw =
            '---\n' +
            'title: "Tagged"\n' +
            'created: 1\n' +
            'updated: 2\n' +
            'pinned: true\n' +
            'tags: ["work", "ideas", \'rust\']\n' +
            '---\n' +
            '\nBody';
        const note = parseNoteMarkdown(raw);
        expect(note.tags).toEqual(['work', 'ideas', 'rust']);
        expect(note.pinned).toBe(true);
    });

    it('falls back to first heading when title is empty', () => {
        const raw =
            '---\n' +
            'title: ""\n' +
            'created: 1\n' +
            'updated: 2\n' +
            'pinned: false\n' +
            'tags: []\n' +
            '---\n' +
            '\n## My Heading\n\nbody';
        const note = parseNoteMarkdown(raw, 'fallback.ki');
        expect(note.title).toBe('My Heading');
    });

    it('falls back to filename when no title and no heading', () => {
        const raw =
            '---\n' +
            'title: ""\n' +
            'created: 0\n' +
            'updated: 0\n' +
            'pinned: false\n' +
            'tags: []\n' +
            '---\n' +
            '\njust paragraph text';
        const note = parseNoteMarkdown(raw, 'shopping-list.ki');
        expect(note.title).toBe('shopping-list');
    });

    it('returns "Untitled note" when title, heading, and filename are all missing', () => {
        const note = parseNoteMarkdown('plain markdown body');
        expect(note.title).toBe('Untitled note');
    });

    it('handles a file with no frontmatter at all', () => {
        const raw = '# Just a Heading\n\nNo frontmatter here.';
        const note = parseNoteMarkdown(raw);
        expect(note.frontmatterValid).toBe(false);
        expect(note.title).toBe('Just a Heading');
        expect(note.bodyMarkdown).toBe(raw);
        expect(note.createdAt).toBeNull();
        expect(note.updatedAt).toBeNull();
    });

    it('reads legacy ISO timestamps from quick notes', () => {
        const note = parseNoteMarkdown(
            '---\ncreated: 2026-07-30T12:00:00.000Z\nupdated: 2026-07-30T13:00:00.000Z\n---\nbody',
        );

        expect(note.createdAt).toBe(Date.parse('2026-07-30T12:00:00.000Z'));
        expect(note.updatedAt).toBe(Date.parse('2026-07-30T13:00:00.000Z'));
    });

    it('handles malformed frontmatter without crashing', () => {
        const raw = '---\nthis is not yaml at all\nno colons\n---\nbody';
        const note = parseNoteMarkdown(raw, 'broken.ki');
        // The block was structurally `--- ... ---`, so it's stripped from
        // the body — but no recognized keys means frontmatterValid=false.
        expect(note.frontmatterValid).toBe(false);
        expect(note.title).toBe('broken'); // falls through to filename
    });

    it('treats created: 0 as null (no 1970 dates)', () => {
        const raw =
            '---\n' +
            'title: "Z"\n' +
            'created: 0\n' +
            'updated: 0\n' +
            'pinned: false\n' +
            'tags: []\n' +
            '---\n' +
            'body';
        const note = parseNoteMarkdown(raw);
        expect(note.createdAt).toBeNull();
        expect(note.updatedAt).toBeNull();
    });

    it('survives empty input', () => {
        const note = parseNoteMarkdown('', 'x.ki');
        expect(note.title).toBe('x');
        expect(note.bodyMarkdown).toBe('');
        expect(note.frontmatterValid).toBe(false);
    });

    it('survives null-ish input', () => {
        // @ts-expect-error — caller may pass undefined from a missing fetch.
        const note = parseNoteMarkdown(undefined, 'y.ki');
        expect(note.title).toBe('y');
        expect(note.bodyMarkdown).toBe('');
    });

    it('handles CRLF line endings (Windows)', () => {
        const raw =
            '---\r\n' +
            'title: "windows"\r\n' +
            'created: 1\r\n' +
            'updated: 2\r\n' +
            'pinned: false\r\n' +
            'tags: []\r\n' +
            '---\r\n' +
            '\r\n# Heading\r\n\r\nbody';
        const note = parseNoteMarkdown(raw, 'win.ki');
        expect(note.title).toBe('windows');
        expect(note.bodyMarkdown.startsWith('# Heading')).toBe(true);
    });
});
