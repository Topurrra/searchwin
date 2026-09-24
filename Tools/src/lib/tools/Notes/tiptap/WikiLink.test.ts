import { describe, it, expect } from 'vitest';
import MarkdownIt from 'markdown-it';
import { extractWikiLinks, isUnresolvedWikiLink, registerWikiLinkRule } from './WikiLink';

/*
  Guards the markdown-it half of WikiLink.ts — the fiddly half.

  These tests drive the SAME markdown-it that tiptap-markdown uses, with the
  same rule and the same registration order, so they fail if either the rule
  or its position in the chain regresses.

  The ProseMirror/TipTap half (node schema, renderHTML, autocomplete) isn't
  covered here — that needs a DOM editor instance and is manual-QA territory.
*/

/** Kept in sync with WIKILINK_RE in WikiLink.ts. */
const WIKILINK_RE = /^\[\[([^\[\]|]+)(?:\|([^\[\]|]*))?\]\]/;

function wikiLinkRule(state: any, silent: boolean): boolean {
    const src: string = state.src;
    const pos: number = state.pos;
    if (src.charCodeAt(pos) !== 0x5b || src.charCodeAt(pos + 1) !== 0x5b) return false;
    const match = WIKILINK_RE.exec(src.slice(pos));
    if (!match) return false;
    const title = match[1].trim();
    if (!title) return false;
    if (!silent) {
        const token = state.push('wikilink', '', 0);
        token.meta = { title, alias: match[2]?.trim() || null };
    }
    state.pos += match[0].length;
    return true;
}

function render(src: string, order: 'before' | 'push' = 'before'): string {
    const md = new MarkdownIt();
    if (order === 'before') md.inline.ruler.before('link', 'wikilink', wikiLinkRule);
    else md.inline.ruler.push('wikilink', wikiLinkRule);
    md.renderer.rules.wikilink = (t: any, i: number) =>
        `<WIKI:${t[i].meta.title}${t[i].meta.alias ? '|' + t[i].meta.alias : ''}>`;
    return md.render(src).trim().replace(/\n/g, ' ');
}

/** Kept in sync with escapeHtmlAttr/renderWikiLinkToken in WikiLink.ts. */
function escapeHtmlAttr(v: string): string {
    return v
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');
}

/** Renders through the REAL HTML bridge, i.e. what parseHTML() will receive. */
function renderHtmlBridge(src: string): string {
    const md = new MarkdownIt();
    md.inline.ruler.before('link', 'wikilink', wikiLinkRule);
    md.renderer.rules.wikilink = (t: any, i: number) => {
        const { title, alias } = t[i].meta;
        const aliasAttr = alias ? ` data-alias="${escapeHtmlAttr(alias)}"` : '';
        return `<span data-wikilink data-title="${escapeHtmlAttr(title)}"${aliasAttr}>${escapeHtmlAttr(alias || title)}</span>`;
    };
    return md.render(src).trim().replace(/\n/g, ' ');
}

describe('wikilink markdown-it rule', () => {
    it('registers once when tiptap parses more than one Markdown value', () => {
        const md = new MarkdownIt();
        registerWikiLinkRule(md);
        registerWikiLinkRule(md);

        expect((md.inline.ruler as any).__rules__.filter((rule: any) => rule.name === 'wikilink')).toHaveLength(1);
    });

    it('only marks known-missing links unresolved', () => {
        expect(isUnresolvedWikiLink('Roadmap', () => false)).toBe(true);
        expect(isUnresolvedWikiLink('Roadmap', () => null)).toBe(false);
        expect(isUnresolvedWikiLink('', () => false)).toBe(false);
    });

    it('extracts aliases while leaving code literals alone', () => {
        expect(extractWikiLinks('See [[Roadmap|the plan]] and `[[not a link]]`.')).toEqual([
            { title: 'Roadmap', alias: 'the plan' },
        ]);
    });

    it('parses [[Title]] and [[Title|alias]]', () => {
        expect(render('See [[Product Roadmap]] here.')).toContain('<WIKI:Product Roadmap>');
        expect(render('See [[Roadmap|the roadmap]] here.')).toContain('<WIKI:Roadmap|the roadmap>');
    });

    it('leaves ordinary markdown links alone', () => {
        expect(render('A [real link](https://x.com) here.')).toContain('<a href="https://x.com">');
    });

    it('does not touch [[...]] inside a code span', () => {
        // markdown-it tokenizes code before inline rules, so this needs no
        // escaping logic of our own — this test proves that assumption.
        expect(render('Literal `[[Not A Link]]` here.')).toContain('<code>[[Not A Link]]</code>');
    });

    it('ignores an empty or unclosed link', () => {
        expect(render('Bad [[]] here.')).not.toContain('<WIKI:');
        expect(render('Oops [[Title here.')).not.toContain('<WIKI:');
    });

    /* THE REASON FOR ruler.before('link').
     *
     * If someone "tidies" WikiLink.ts to use ruler.push(), these two inputs
     * silently start rendering as ordinary links and the wikilink is
     * destroyed. This is the regression that test exists to catch. */
    describe('rule ordering vs the built-in link rule', () => {
        it('wins against inline link syntax directly after the wikilink', () => {
            expect(render('See [[Title]](https://x.com) here.', 'before')).toContain('<WIKI:Title>');
            // Demonstrates the failure the ordering prevents:
            expect(render('See [[Title]](https://x.com) here.', 'push')).toContain(
                '<a href="https://x.com">[Title]</a>',
            );
        });

        it('wins against a full reference link directly after the wikilink', () => {
            const src = 'See [[Title]][ref] here.\n\n[ref]: https://x.com';
            expect(render(src, 'before')).toContain('<WIKI:Title>');
            expect(render(src, 'push')).toContain('<a href="https://x.com">[Title]</a>');
        });

        it('a matching reference DEFINITION elsewhere is NOT a hazard either way', () => {
            // Recorded because it's the intuitive-sounding trap and it is not
            // real — the parsed label is `[Title]`, which never normalizes
            // onto the `title` ref. Don't add defensive code for this.
            const src = 'See [[Title]] here.\n\n[Title]: https://example.com';
            expect(render(src, 'before')).toContain('<WIKI:Title>');
            expect(render(src, 'push')).toContain('<WIKI:Title>');
        });
    });

    /* tiptap-markdown parses by rendering markdown to HTML and letting TipTap's
     * parseHTML() build nodes from it — there is no token->node mapping. So the
     * HTML shape below IS the contract with WikiLink's parseHTML(). */
    describe('HTML bridge (what parseHTML actually receives)', () => {
        it('emits a span parseHTML can match, carrying title and alias', () => {
            expect(renderHtmlBridge('See [[Product Roadmap]].')).toContain(
                '<span data-wikilink data-title="Product Roadmap">Product Roadmap</span>',
            );
            expect(renderHtmlBridge('See [[Roadmap|the plan]].')).toContain(
                '<span data-wikilink data-title="Roadmap" data-alias="the plan">the plan</span>',
            );
        });

        it('escapes a title that would otherwise break out of the attribute', () => {
            // Note titles are user text going into an HTML attribute. Unescaped,
            // a note titled `" onmouseover=x` injects into our own editor.
            const html = renderHtmlBridge('See [[a" onmouseover="evil]].');
            expect(html).not.toContain('onmouseover="evil"');
            expect(html).toContain('&quot;');
        });
    });
});
