/*
  WikiLink — `[[Note Title]]` / `[[Note Title|alias]]` as a first-class TipTap node.

  WHY A NODE, NOT A MARK: a wikilink is a chip you pick from an autocomplete
  list (like @mention), not arbitrary text you select and decorate. TipTap's own
  Mention extension is built the same way.

  WHY NOT REUSE THE INSTALLED Link MARK: modelling this as `href="ki-note://X"`
  would technically reuse a dependency we already have, but tiptap-markdown
  would serialize it back as `[X](ki-note://X)` — not `[[X]]`. That silently
  breaks both Obsidian portability and the project's own "plaintext .ki, yours
  forever, openable in any editor" promise (see notes.rs's module docs). The
  whole point of the format is that another tool can read it.

  HOW PARSING ACTUALLY WORKS HERE (this surprised me, so it's written down):
  tiptap-markdown does NOT use prosemirror-markdown's token->node mapping. Its
  MarkdownParser does `md.render(markdown)` to get HTML, then hands that HTML to
  TipTap, which builds nodes via each node's parseHTML(). So the markdown-it
  side must RENDER HTML that parseHTML() below can match — a token spec would do
  nothing. Verified by reading tiptap-markdown 0.9.0's src/parse/MarkdownParser.js
  and its shipped image/link extensions.

  RESOLVED-NESS IS NEVER STORED. Whether `[[X]]` points at a real note is a
  function of the notes list, which changes as notes are created/renamed/deleted
  elsewhere. Baking it into a node attr would guarantee stale styling, so it's
  computed live through the `resolve` callback passed via .configure().
*/
import { Node, mergeAttributes } from '@tiptap/core';
import { Plugin } from '@tiptap/pm/state';
import { Decoration, DecorationSet } from '@tiptap/pm/view';
import MarkdownIt from 'markdown-it';

export interface WikiLinkReference {
    title: string;
    alias: string | null;
}

export interface WikiLinkOptions {
    /** Live lookup: does a note with this title exist?
     *
     *  Returns `null` for "can't know" — the sticky-note window has no notes
     *  list to check against. Unknown renders a neutral chip; an unknown answer
     *  must not be displayed as a confident "broken".
     *
     *  Tri-state rather than a nullable callback so the host can pass one stable
     *  closure and decide per call, instead of the option itself flipping.
     *  (Clicks are handled by the host's own DOM listener, not an option here —
     *  it already owns a click handler for external links and the two must not
     *  race for the same event.) */
    resolve: (title: string) => boolean | null;
    HTMLAttributes: Record<string, unknown>;
}

/** `[[Title]]` or `[[Title|alias]]`. Rejects an empty title and anything
 *  containing `[` or `]`, so a stray bracket can't swallow the rest of a line. */
const WIKILINK_RE = /^\[\[([^\[\]|]+)(?:\|([^\[\]|]*))?\]\]/;

export const WikiLink = Node.create<WikiLinkOptions>({
    name: 'wikiLink',
    group: 'inline',
    inline: true,
    atom: true,
    selectable: true,

    addOptions() {
        return {
            // Unknown by default: an unconfigured editor must not claim every
            // link is broken.
            resolve: () => null,
            HTMLAttributes: {},
        };
    },

    addAttributes() {
        return {
            title: {
                default: '',
                parseHTML: (el) => el.getAttribute('data-title') ?? '',
                renderHTML: (attrs) => ({ 'data-title': attrs.title }),
            },
            alias: {
                default: null,
                parseHTML: (el) => el.getAttribute('data-alias'),
                renderHTML: (attrs) => (attrs.alias ? { 'data-alias': attrs.alias } : {}),
            },
        };
    },

    parseHTML() {
        // Matches both what our markdown-it renderer emits (markdown -> HTML)
        // and what renderHTML emits (copy/paste round trip).
        return [{ tag: 'span[data-wikilink]' }];
    },

    renderHTML({ node, HTMLAttributes }) {
        const title = String(node.attrs.title ?? '');
        return [
            'span',
            mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, {
                'data-wikilink': '',
                class: 'wikilink',
                // Tells the user WHY a link looks dead instead of leaving them
                // to guess at the styling.
                title,
            }),
            node.attrs.alias || title,
        ];
    },

    addProseMirrorPlugins() {
        return [
            new Plugin({
                props: {
                    decorations: (state) => {
                        const decorations: Decoration[] = [];
                        state.doc.descendants((node, pos) => {
                            if (node.type.name !== 'wikiLink') return;
                            const title = String(node.attrs.title ?? '');
                            if (!isUnresolvedWikiLink(title, this.options.resolve)) return;
                            decorations.push(
                                Decoration.node(pos, pos + node.nodeSize, {
                                    class: 'is-unresolved',
                                    title: `${title} - no note with this title yet`,
                                }),
                            );
                        });
                        return DecorationSet.create(state.doc, decorations);
                    },
                },
            }),
        ];
    },

    renderText({ node }) {
        // Plain-text export (and copy-as-text) keeps the source syntax, so a
        // note exported to .txt still shows its links rather than dropping them.
        const { title, alias } = node.attrs as { title: string; alias?: string };
        return alias ? `[[${title}|${alias}]]` : `[[${title}]]`;
    },

    addStorage() {
        return {
            markdown: {
                serialize(
                    state: { text: (s: string, escape?: boolean) => void },
                    node: { attrs: { title: string; alias?: string | null } },
                ) {
                    const { title, alias } = node.attrs;
                    // escape=false: the brackets ARE the syntax. Letting the
                    // serializer escape them emits \[\[Title\]\], which no
                    // other markdown tool would recognise.
                    state.text(alias ? `[[${title}|${alias}]]` : `[[${title}]]`, false);
                },
                parse: {
                    setup(md: MarkdownIt) {
                        registerWikiLinkRule(md);
                    },
                },
            },
        };
    },
});

/** Extract links through the same markdown-it rule the editor uses, so code
 * spans and fences remain literal rather than becoming false link references. */
export function extractWikiLinks(markdown: string): WikiLinkReference[] {
    const md = new MarkdownIt();
    registerWikiLinkRule(md);
    const links: WikiLinkReference[] = [];

    for (const block of md.parse(markdown, {})) {
        for (const token of block.children ?? []) {
            if (token.type !== 'wikilink') continue;
            const title = String(token.meta?.title ?? '').trim();
            if (!title) continue;
            links.push({ title, alias: token.meta?.alias ?? null });
        }
    }
    return links;
}

const registeredMarkdownIt = new WeakSet<MarkdownIt>();

/** True only for a known-missing note. `null` remains visually neutral. */
export function isUnresolvedWikiLink(
    title: string,
    resolve: (title: string) => boolean | null,
): boolean {
    return Boolean(title) && resolve(title) === false;
}

/** `tiptap-markdown` invokes extension setup for every Markdown parse. */
export function registerWikiLinkRule(md: MarkdownIt): void {
    if (registeredMarkdownIt.has(md)) return;
    md.inline.ruler.before('link', 'wikilink', wikiLinkRule);
    md.renderer.rules.wikilink = renderWikiLinkToken;
    registeredMarkdownIt.add(md);
}

/** markdown-it inline rule for `[[Title]]` / `[[Title|alias]]`.
 *
 *  MUST be registered BEFORE the built-in `link` rule. Verified against the
 *  real markdown-it 14.1.1 this app ships (see WikiLink.test.ts), which found
 *  exactly two inputs where the ordering changes the result — both are a
 *  wikilink immediately followed by link syntax:
 *
 *      [[Title]](https://x.com)   ->  push(): <a href="x.com">[Title]</a>
 *      [[Title]][ref]             ->  push(): <a href="x.com">[Title]</a>
 *
 *  i.e. the built-in link rule claims the `[Title]` inside our brackets and
 *  the wikilink is destroyed. Registering first makes us win the position.
 *
 *  (A matching reference DEFINITION elsewhere in the note — `[Title]: url` —
 *  turns out NOT to matter: both orderings handle it identically, because the
 *  parsed label is `[Title]`, which doesn't normalize onto the `title` ref.
 *  Noted because it's the intuitive-sounding hazard, and it isn't the real one.)
 *
 *  Code spans/fences need no special handling — markdown-it tokenizes those
 *  before generic inline rules run, so a literal `[[x]]` inside backticks is
 *  already shielded. Also verified, not assumed. */
function wikiLinkRule(state: any, silent: boolean): boolean {
    const src: string = state.src;
    const pos: number = state.pos;

    // Cheap bail before the regex — this rule is consulted at every '[' in
    // every note, so the common case (an ordinary link) must stay cheap.
    if (src.charCodeAt(pos) !== 0x5b /* [ */ || src.charCodeAt(pos + 1) !== 0x5b) {
        return false;
    }

    const match = WIKILINK_RE.exec(src.slice(pos));
    if (!match) return false;

    const title = match[1].trim();
    if (!title) return false;
    const alias = match[2]?.trim() || null;

    // silent = markdown-it is only probing whether the rule matches (e.g. for
    // link-inside-link detection); it must not emit tokens.
    if (!silent) {
        const token = state.push('wikilink', '', 0);
        token.meta = { title, alias };
    }
    state.pos += match[0].length;
    return true;
}

/** Renders the token to the HTML that `parseHTML()` above matches — this is
 *  the actual markdown -> node bridge (see the module header). */
function renderWikiLinkToken(tokens: any[], idx: number): string {
    const { title, alias } = tokens[idx].meta as { title: string; alias: string | null };
    // Titles are user text landing in HTML attributes — escape, or a note
    // titled `" onmouseover=` becomes an injection vector into our own editor.
    const esc = escapeHtmlAttr;
    const aliasAttr = alias ? ` data-alias="${esc(alias)}"` : '';
    return `<span data-wikilink data-title="${esc(title)}"${aliasAttr}>${esc(alias || title)}</span>`;
}

function escapeHtmlAttr(value: string): string {
    return value
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');
}
