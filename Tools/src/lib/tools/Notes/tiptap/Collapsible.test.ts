import MarkdownIt from 'markdown-it';
import { describe, expect, it } from 'vitest';
import { Collapsible } from './Collapsible';

describe('Collapsible Markdown setup', () => {
    it('registers its parser once when the editor serializes repeatedly', () => {
        const markdown = new MarkdownIt();
        const setup = (Collapsible.config.addStorage as () => any)().markdown.parse.setup;

        setup(markdown);
        setup(markdown);

        expect(markdown.render(':::details More\nHidden text\n:::')).toContain(
            '<details data-notes-collapsible data-title="More">',
        );
        expect((markdown.block.ruler as any).__rules__.filter((rule: any) => rule.name === 'notes_collapsible')).toHaveLength(1);
    });

    it('supports an initially open block so the new writing target is visible', () => {
        const attributes = (Collapsible.config.addAttributes as () => any)();

        expect(attributes.open.default).toBe(false);
        expect(attributes.open.renderHTML({ open: true })).toEqual({ open: '' });
        expect(attributes.open.renderHTML({ open: false })).toEqual({});
    });
});
