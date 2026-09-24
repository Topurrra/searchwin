import { Extension, type Editor } from '@tiptap/core';
import { PluginKey } from '@tiptap/pm/state';
import Suggestion from '@tiptap/suggestion';
import type { EditorView } from '@tiptap/pm/view';

export interface SlashCommand {
    title: string;
    detail: string;
    keywords?: string[];
    run: (editor: Editor, range: { from: number; to: number }) => void;
}

export interface SlashCommandSuggestionOptions {
    getCommands: () => SlashCommand[];
}

const MAX_ITEMS = 8;
const slashCommandSuggestionPluginKey = new PluginKey('slashCommandSuggestion');
type SuggestionRange = { from: number; to: number };

export function filterSlashCommands(commands: SlashCommand[], query: string): SlashCommand[] {
    const normalized = query.trim().toLocaleLowerCase();
    if (!normalized) return commands.slice(0, MAX_ITEMS);
    return commands
        .filter((command) =>
            [command.title, command.detail, ...(command.keywords ?? [])]
                .join(' ')
                .toLocaleLowerCase()
                .includes(normalized),
        )
        .sort((a, b) => {
            const aPrefix = a.title.toLocaleLowerCase().startsWith(normalized) ? 0 : 1;
            const bPrefix = b.title.toLocaleLowerCase().startsWith(normalized) ? 0 : 1;
            return aPrefix - bPrefix || a.title.localeCompare(b.title);
        })
        .slice(0, MAX_ITEMS);
}

export const SlashCommandSuggestion = Extension.create<SlashCommandSuggestionOptions>({
    name: 'slashCommandSuggestion',

    addOptions() {
        return { getCommands: () => [] };
    },

    addProseMirrorPlugins() {
        let dismissedRange: SuggestionRange | null = null;
        const dismiss = (view: EditorView, range: SuggestionRange) => {
            dismissedRange = { ...range };
            view.dispatch(view.state.tr.setMeta('notes-slash-dismiss', true));
        };

        return [
            Suggestion<SlashCommand>({
                editor: this.editor,
                pluginKey: slashCommandSuggestionPluginKey,
                char: '/',
                startOfLine: false,
                allowSpaces: true,
                allow: ({ state, range }) => {
                    if (dismissedRange) {
                        if (dismissedRange.from === range.from && dismissedRange.to === range.to) return false;
                        dismissedRange = null;
                    }
                    return state.doc.resolve(range.from).parent.type.name === 'paragraph';
                },
                items: ({ query }) => filterSlashCommands(this.options.getCommands(), query),
                command: ({ editor, range, props }) => props.run(editor, range),
                render: () => new SlashCommandPopup(dismiss),
            }),
        ];
    },
});

class SlashCommandPopup {
    private el: HTMLDivElement | null = null;
    private items: SlashCommand[] = [];
    private selected = 0;
    private command: ((item: SlashCommand) => void) | null = null;

    constructor(private readonly dismiss: (view: EditorView, range: SuggestionRange) => void) {}

    onStart = (props: any) => {
        this.items = props.items;
        this.selected = 0;
        this.command = props.command;
        this.el = document.createElement('div');
        this.el.className = 'notes-slash-menu';
        document.body.appendChild(this.el);
        this.paint();
        this.position(props);
    };

    onUpdate = (props: any) => {
        this.items = props.items;
        this.command = props.command;
        if (this.selected >= this.items.length) this.selected = Math.max(0, this.items.length - 1);
        this.paint();
        this.position(props);
    };

    onKeyDown = ({ event, view, range }: { event: KeyboardEvent; view: EditorView; range: SuggestionRange }) => {
        if (event.key === 'Escape') {
            this.dismiss(view, range);
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
            if (item) this.command?.(item);
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
            empty.className = 'notes-slash-empty';
            empty.textContent = 'No matching block';
            this.el.appendChild(empty);
            return;
        }
        this.items.forEach((item, index) => {
            const row = document.createElement('button');
            row.type = 'button';
            row.className = index === this.selected ? 'notes-slash-option is-active' : 'notes-slash-option';
            const title = document.createElement('span');
            title.className = 'notes-slash-option-title';
            title.textContent = item.title;
            const detail = document.createElement('span');
            detail.className = 'notes-slash-option-detail';
            detail.textContent = item.detail;
            row.append(title, detail);
            row.addEventListener('mousedown', (event) => {
                event.preventDefault();
                this.command?.(item);
            });
            this.el!.appendChild(row);
        });
    }

    private position(props: { clientRect?: (() => DOMRect | null) | null }) {
        if (!this.el || !props.clientRect) return;
        const rect = props.clientRect();
        if (!rect) return;
        const height = this.el.offsetHeight || 220;
        const top = window.innerHeight - rect.bottom < height + 8 ? rect.top - height - 6 : rect.bottom + 6;
        this.el.style.left = `${Math.round(Math.max(8, Math.min(rect.left, window.innerWidth - 280)))}px`;
        this.el.style.top = `${Math.round(Math.max(6, top))}px`;
    }
}
