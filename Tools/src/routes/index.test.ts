// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import Index from './+page.svelte';
import { catalog, openTool } from '$lib/searchTools';
import { offInSearch } from '$lib/offInSearch';

describe('search://tools', () => {
    let page: ReturnType<typeof mount> | undefined;
    afterEach(() => {
        if (page) unmount(page);
        page = undefined;
        document.body.innerHTML = '';
    });

    const open = () => {
        page = mount(Index, { target: document.body });
        flushSync();
    };
    const packs = () => [...document.querySelectorAll('section h2')].map((h) => h.textContent);
    const cards = () => [...document.querySelectorAll<HTMLAnchorElement>('a.tool')].map((a) => a.getAttribute('href'));

    it("lists Search's tools by pack, in Search's order", () => {
        open();
        expect(packs()).toEqual(['Utilities', 'Files', 'Privacy', 'Images', 'Documents', 'Development', 'Media', 'Focus']);
        const privacy = document.querySelector('section[aria-label="Privacy"]')!.textContent!;
        // Workspace kept these as pages of their own; in Search they're tools.
        expect(privacy).toContain('Privacy Audit');
        expect(document.querySelector('section[aria-label="Focus"]')!.textContent).toContain('Time Tracker');
    });

    it('leaves out everything on the one list, and the field offers exactly what it shows', () => {
        open();
        for (const id of Object.keys(offInSearch)) expect(cards()).not.toContain(`#/tool/${id}`);
        expect(cards()).toEqual(catalog().map((tool) => `#/tool/${tool.id}`));
    });

    it('withholds unfinished recorder and reminders from both entry points', () => {
        open();
        for (const id of ['screen-recorder', 'reminders']) {
            expect(cards()).not.toContain(`#/tool/${id}`);
            expect(catalog().map((tool) => tool.id)).not.toContain(id);
            expect(openTool(id)).toEqual({ refused: expect.any(String) });
        }
    });

    it('narrows as you type and opens the first on Enter, through its link', () => {
        open();
        const filter = document.querySelector<HTMLInputElement>('input.filter')!;
        filter.value = 'hash';
        filter.dispatchEvent(new Event('input'));
        flushSync();
        expect(cards()).toEqual(['#/tool/hash-check']);
        let followed = '';
        // The router takes a link; an edit to location.hash would reload the page.
        document.addEventListener('click', (e) => (followed = (e.target as HTMLAnchorElement).getAttribute('href') ?? ''), { once: true });
        filter.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
        expect(followed).toBe('#/tool/hash-check');
    });
});

describe('a tool\'s address', () => {
    it('opens a tool, and says why for one Search leaves out', () => {
        expect(openTool('hash-check')).toMatchObject({ name: 'Hash Check' });
        expect(openTool('privacy-audit')).toMatchObject({ name: 'Privacy Audit' });
        expect(openTool('word-converter')).toEqual({ refused: offInSearch['word-converter'] });
        expect(openTool('clipboard-history')).toEqual({ refused: offInSearch['clipboard-history'] });
        expect(openTool('nothing-here')).toEqual({ refused: 'There\'s no tool called “nothing-here”.' });
    });
});
