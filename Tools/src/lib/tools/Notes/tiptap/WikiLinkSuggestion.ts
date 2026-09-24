/*
  `[[` autocomplete for WikiLink, built on @tiptap/suggestion — the same
  first-party utility behind TipTap's own Mention extension.

  The popup is a plain DOM list rather than a mounted Svelte component:
  Suggestion hands us imperative lifecycle hooks (onStart/onUpdate/onExit) that
  don't map onto Svelte 5's declarative mount, and the list is 8 rows of text.
  A component here would mean manual mount/unmount plumbing for no gain.

  Candidates come from the notes list already in memory — no IPC per keystroke.
*/
import { Extension } from '@tiptap/core';
import { PluginKey } from '@tiptap/pm/state';
import type { EditorView } from '@tiptap/pm/view';
import Suggestion from '@tiptap/suggestion';

export interface WikiLinkCandidate {
    title: string;
}

export interface WikiLinkSuggestionOptions {
    /** Current note titles. Called per query, so it always sees fresh data. */
    getTitles: () => string[];
}

const MAX_ITEMS = 8;
const wikiLinkSuggestionPluginKey = new PluginKey('wikiLinkSuggestion');
type SuggestionRange = { from: number; to: number };

export const WikiLinkSuggestion = Extension.create<WikiLinkSuggestionOptions>({
    name: 'wikiLinkSuggestion',

    addOptions() {
        return { getTitles: () => [] };
    },

    addProseMirrorPlugins() {
        const getTitles = () => this.options.getTitles();
        let dismissedRange: SuggestionRange | null = null;
        const dismiss = (view: EditorView, range: SuggestionRange) => {
            dismissedRange = { ...range };
            view.dispatch(view.state.tr.setMeta('notes-wikilink-dismiss', true));
        };

        return [
            Suggestion<WikiLinkCandidate>({
                editor: this.editor,
                pluginKey: wikiLinkSuggestionPluginKey,
                char: '[[',
                // The trigger is two chars and markdown is full of brackets, so
                // only fire at a word boundary — mid-word `a[[` is far more
                // likely to be code or an array index than a link.
                allowSpaces: true,
                startOfLine: false,
                allow: ({ range }) => {
                    if (!dismissedRange) return true;
                    if (dismissedRange.from === range.from && dismissedRange.to === range.to) return false;
                    dismissedRange = null;
                    return true;
                },

                items: ({ query }) => {
                    const q = query.trim().toLowerCase();
                    const titles = getTitles();
                    const pool = q
                        ? titles.filter((t) => t.toLowerCase().includes(q))
                        : titles;
                    // Prefix matches first — typing "road" should surface
                    // "Roadmap" above "Product Roadmap".
                    if (q) {
                        pool.sort((a, b) => {
                            const ap = a.toLowerCase().startsWith(q) ? 0 : 1;
                            const bp = b.toLowerCase().startsWith(q) ? 0 : 1;
                            return ap - bp || a.localeCompare(b);
                        });
                    }
                    return pool.slice(0, MAX_ITEMS).map((title) => ({ title }));
                },

                command: ({ editor, range, props }) => {
                    editor
                        .chain()
                        .focus()
                        .insertContentAt(range, [
                            { type: 'wikiLink', attrs: { title: props.title, alias: null } },
                            // Trailing space, or the cursor is stuck welded to
                            // an atom node and typing feels broken.
                            { type: 'text', text: ' ' },
                        ])
                        .run();
                },

                render: () => new SuggestionPopup(dismiss),
            }),
        ];
    },
});

/** Imperative popup driven by Suggestion's lifecycle. */
class SuggestionPopup {
    private el: HTMLDivElement | null = null;
    private items: WikiLinkCandidate[] = [];
    private selected = 0;
    private command: ((item: WikiLinkCandidate) => void) | null = null;

    constructor(private readonly dismiss: (view: EditorView, range: SuggestionRange) => void) {}

    onStart = (props: any) => {
        this.items = props.items;
        this.selected = 0;
        this.command = props.command;

        this.el = document.createElement('div');
        this.el.className = 'wikilink-menu';
        document.body.appendChild(this.el);
        this.paint();
        this.position(props);
    };

    onUpdate = (props: any) => {
        this.items = props.items;
        this.command = props.command;
        // The list can shrink under a stale index; clamp rather than trusting
        // the key handler to have kept up.
        if (this.selected >= this.items.length) this.selected = Math.max(0, this.items.length - 1);
        this.paint();
        this.position(props);
    };

    onKeyDown = (props: { event: KeyboardEvent; view: EditorView; range: SuggestionRange }) => {
        const { event } = props;
        if (event.key === 'Escape') {
            this.dismiss(props.view, props.range);
            return true;
        }
        if (!this.items.length) return false;

        if (event.key === 'ArrowDown') {
            this.selected = (this.selected + 1) % this.items.length;
            this.paint();
            return true;
        }
        if (event.key === 'ArrowUp') {
            this.selected = (this.selected - 1 + this.items.length) % this.items.length;
            this.paint();
            return true;
        }
        if (event.key === 'Enter' || event.key === 'Tab') {
            const item = this.items[this.selected];
            if (item && this.command) this.command(item);
            return true;
        }
        return false;
    };

    onExit = () => {
        this.el?.remove();
        this.el = null;
        this.command = null;
    };

    private paint() {
        if (!this.el) return;
        this.el.replaceChildren();

        if (!this.items.length) {
            const empty = document.createElement('div');
            empty.className = 'wikilink-menu-empty';
            empty.textContent = 'No matching note';
            this.el.appendChild(empty);
            return;
        }

        this.items.forEach((item, i) => {
            const row = document.createElement('button');
            row.type = 'button';
            row.className = i === this.selected ? 'wikilink-opt is-active' : 'wikilink-opt';
            row.textContent = item.title;
            // mousedown, not click: the editor blurs first and would tear the
            // popup down before click ever lands.
            row.addEventListener('mousedown', (e) => {
                e.preventDefault();
                this.command?.(item);
            });
            this.el!.appendChild(row);
        });
    }

    private position(props: { clientRect?: (() => DOMRect | null) | null }) {
        if (!this.el || !props.clientRect) return;
        const rect = props.clientRect();
        if (!rect) return;

        // Flip above the caret when there isn't room below, so the list never
        // hangs off the bottom of a short editor pane.
        const menuHeight = this.el.offsetHeight || 200;
        const below = window.innerHeight - rect.bottom;
        const top = below < menuHeight + 8 ? rect.top - menuHeight - 6 : rect.bottom + 6;

        this.el.style.left = `${Math.round(Math.min(rect.left, window.innerWidth - 240))}px`;
        this.el.style.top = `${Math.round(Math.max(4, top))}px`;
    }
}
