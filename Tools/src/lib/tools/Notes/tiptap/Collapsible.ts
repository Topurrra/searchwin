/* A portable disclosure block: :::details Title ... ::: */
import { mergeAttributes, Node } from '@tiptap/core';
import MarkdownIt from 'markdown-it';

const DEFAULT_TITLE = 'Details';
const configuredMarkdownIt = new WeakSet<MarkdownIt>();

export const Collapsible = Node.create({
    name: 'collapsible',
    group: 'block',
    content: 'block+',
    defining: true,

    addAttributes() {
        return {
            title: {
                default: DEFAULT_TITLE,
                parseHTML: (element: HTMLElement) => safeTitle(element.getAttribute('data-title') ?? ''),
                renderHTML: (attrs: { title?: unknown }) => ({
                    'data-title': safeTitle(String(attrs.title ?? '')),
                }),
            },
            open: {
                default: false,
                parseHTML: (element: HTMLElement) => element.hasAttribute('open'),
                renderHTML: (attrs: { open?: boolean }) => (attrs.open ? { open: '' } : {}),
            },
        };
    },

    parseHTML() {
        return [
            {
                tag: 'details[data-notes-collapsible]',
                contentElement: 'div[data-notes-collapsible-content]',
                getAttrs: (element: HTMLElement) => ({
                    title: safeTitle(element.getAttribute('data-title') ?? ''),
                }),
            },
        ];
    },

    renderHTML({ node, HTMLAttributes }) {
        const title = safeTitle(String(node.attrs.title ?? ''));
        return [
            'details',
            mergeAttributes(HTMLAttributes, {
                'data-notes-collapsible': '',
                'data-title': title,
                class: 'notes-collapsible',
            }),
            ['summary', { contenteditable: 'false' }, title],
            ['div', { 'data-notes-collapsible-content': '', class: 'notes-collapsible-content' }, 0],
        ];
    },

    addStorage() {
        return {
            markdown: {
                serialize(state: any, node: any) {
                    // ponytail: disclosure state is transient UI state; persist it only with a requested per-note view state.
                    state.write(`:::details ${safeTitle(String(node.attrs.title ?? ''))}\n`);
                    state.renderContent(node);
                    state.ensureNewLine();
                    state.write(':::');
                    state.closeBlock(node);
                },
                parse: {
                    setup(md: MarkdownIt) {
                        if (configuredMarkdownIt.has(md)) return;
                        configuredMarkdownIt.add(md);
                        md.block.ruler.before('fence', 'notes_collapsible', collapsibleRule, {
                            alt: ['paragraph', 'reference', 'blockquote', 'list'],
                        });
                        md.renderer.rules.notes_collapsible = (tokens, index, _options, env) => {
                            const token = tokens[index];
                            const title = escapeHtml(safeTitle(String(token.meta?.title ?? '')));
                            return `<details data-notes-collapsible data-title="${title}"><summary>${title}</summary><div data-notes-collapsible-content>${md.render(token.content, env)}</div></details>\n`;
                        };
                    },
                },
            },
        };
    },
});

function collapsibleRule(state: any, startLine: number, endLine: number, silent: boolean): boolean {
    const match = /^:::details(?:\s+(.+?))?\s*$/i.exec(lineAt(state, startLine));
    if (!match) return false;
    let closeLine = startLine + 1;
    while (closeLine < endLine && lineAt(state, closeLine).trim() !== ':::') closeLine++;
    if (closeLine === endLine) return false;
    if (silent) return true;
    const token = state.push('notes_collapsible', 'details', 0);
    token.meta = { title: safeTitle(match[1] ?? '') };
    token.content = state.getLines(startLine + 1, closeLine, state.blkIndent, false);
    state.line = closeLine + 1;
    return true;
}

function lineAt(state: any, line: number): string {
    return state.src.slice(state.bMarks[line] + state.tShift[line], state.eMarks[line]);
}

function safeTitle(value: string): string {
    return value.replace(/[\r\n]+/g, ' ').trim().slice(0, 120) || DEFAULT_TITLE;
}

function escapeHtml(value: string): string {
    return value
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');
}
