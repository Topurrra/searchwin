import { Extension } from '@tiptap/core';
import { Plugin } from '@tiptap/pm/state';
import { Decoration, DecorationSet } from '@tiptap/pm/view';

export type CalloutKind = 'note' | 'tip' | 'warning' | 'file';

export function calloutKindFromText(value: string): { kind: CalloutKind; markerLength: number } | null {
    const match = /^\s*\[!(NOTE|TIP|WARNING|FILE)\]\s*/i.exec(value);
    if (!match) return null;
    return { kind: match[1].toLocaleLowerCase() as CalloutKind, markerLength: match[0].length };
}

/** Adds presentation to a deliberately ordinary Markdown blockquote. */
export const CalloutBlocks = Extension.create({
    name: 'calloutBlocks',

    addProseMirrorPlugins() {
        return [
            new Plugin({
                props: {
                    decorations(state) {
                        const decorations: Decoration[] = [];
                        state.doc.descendants((node, pos) => {
                            if (node.type.name !== 'blockquote') return;
                            const first = node.firstChild;
                            if (!first?.isTextblock) return;
                            const callout = calloutKindFromText(first.textContent);
                            if (!callout) return;
                            decorations.push(
                                Decoration.node(pos, pos + node.nodeSize, {
                                    class: `notes-callout notes-callout-${callout.kind}`,
                                }),
                                Decoration.inline(pos + 2, pos + 2 + callout.markerLength, {
                                    class: 'notes-callout-marker',
                                }),
                            );
                        });
                        return DecorationSet.create(state.doc, decorations);
                    },
                },
            }),
        ];
    },
});
