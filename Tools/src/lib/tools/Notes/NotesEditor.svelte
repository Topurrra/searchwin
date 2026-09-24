<script lang="ts">
    /*
      NotesEditor — a TipTap (ProseMirror) WYSIWYG editor that round-trips
      Markdown via tiptap-markdown, so the underlying `.ki` file stays valid,
      portable Markdown (no lock-in).

      Mounted ONE per open note: the parent wraps this in `{#key note.path}`, so
      each note gets a fresh editor instance — no mid-typing content resets and
      no cursor jumps. TipTap is a vanilla `Editor` (the official Svelte wrapper
      targets Svelte 4); we drive it directly through onMount/onDestroy, which is
      the supported Svelte-5-runes pattern.

      TipTap is only bundled into the lazy-loaded Notes screen chunk, so it never
      touches the app's cold-start cost.
    */
    import { onMount, onDestroy } from 'svelte';
    import { Editor, generateJSON, mergeAttributes } from '@tiptap/core';
    import StarterKit from '@tiptap/starter-kit';
    import Placeholder from '@tiptap/extension-placeholder';
    import TaskList from '@tiptap/extension-task-list';
    import TaskItem from '@tiptap/extension-task-item';
    import Table from '@tiptap/extension-table';
    import TableRow from '@tiptap/extension-table-row';
    import TableHeader from '@tiptap/extension-table-header';
    import TableCell from '@tiptap/extension-table-cell';
    import Link from '@tiptap/extension-link';
    import TiptapImage from '@tiptap/extension-image';
    import { defaultMarkdownSerializer } from '@tiptap/pm/markdown';
    import { Markdown } from 'tiptap-markdown';
    import { WikiLink } from './tiptap/WikiLink';
    import { WikiLinkSuggestion } from './tiptap/WikiLinkSuggestion';
    import { CalloutBlocks } from './tiptap/CalloutBlocks';
    import { Collapsible } from './tiptap/Collapsible';
    import { SlashCommandSuggestion, type SlashCommand } from './tiptap/SlashCommandSuggestion';
    import {
        decodeHtmlEntitiesOnce,
        looksLikeEscapedHtml,
        stripOuterMarkdownFence,
        sanitizeImportedMarkdown,
    } from '$lib/stores/notes';
    import { openPath, openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';
    import { convertFileSrc, invoke } from '@tauri-apps/api/core';
    import { open as openFileDialog } from '@tauri-apps/plugin-dialog';
    import { toast } from '$lib/stores/toasts';
    import {
        Bold,
        Italic,
        Strikethrough,
        Heading,
        List,
        ListOrdered,
        ListChecks,
        Quote,
        Code,
        Table as TableIcon,
        Image as ImageIcon,
        Paperclip,
        MessageSquare,
        Lightbulb,
        TriangleAlert,
        ChevronsUpDown,
        Plus,
        Trash2,
    } from '@lucide/svelte';

    let {
        value = '',
        onChange,
        table = true,
        autofocus = false,
        noteTitles,
        resolveNote,
        onNavigateNote,
        headingTarget,
        editable = true,
        showToolbar = true,
    }: {
        value?: string;
        onChange?: (markdown: string) => void;
        table?: boolean;
        autofocus?: boolean;
        /** Titles offered by the `[[` autocomplete. Omit where there's no
         *  notes list (the sticky-note window) — the trigger then finds
         *  nothing, which is honest rather than empty-but-implying-broken. */
        noteTitles?: () => string[];
        /** Live "does a note with this title exist?". Omit where unknowable:
         *  wikilinks then render neutral instead of all-unresolved. */
        resolveNote?: (title: string) => boolean;
        /** Click a resolved wikilink chip. Omit to make chips inert. */
        onNavigateNote?: (title: string) => void;
        /** A requested heading index from the surrounding note context drawer. */
        headingTarget?: { index: number; request: number } | null;
        /** Split previews reuse the schema but must never enter the save pipeline. */
        editable?: boolean;
        showToolbar?: boolean;
    } = $props();

    let element: HTMLDivElement;
    // TipTap mutates its instance during every transaction. Keep that external
    // object raw; `tick` below is the explicit UI reactivity bridge.
    let editor = $state.raw<Editor | null>(null);
    /** Bumped on every transaction so toolbar active-states stay reactive. */
    let tick = $state(0);
    let lastHeadingRequest = -1;

    $effect(() => {
        const target = headingTarget;
        if (!editor || !target || target.request === lastHeadingRequest) return;
        lastHeadingRequest = target.request;
        const heading = editor.view.dom.querySelectorAll<HTMLElement>('h1, h2, h3, h4, h5, h6')[
            target.index
        ];
        if (!heading) return;
        const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
        heading.scrollIntoView({ behavior: reducedMotion ? 'auto' : 'smooth', block: 'start' });
    });

    /*
      Does this pasted plain-text look like Markdown worth rendering?

      tiptap-markdown ships `transformPastedText`, but ProseMirror only honours
      it when the clipboard is plain-text-ONLY — the moment a source also offers
      an HTML flavour (most chat apps, rendered Markdown views, browsers) it's
      skipped and the raw `# heading` lands as literal text. So we drive paste
      ourselves: if the text carries Markdown syntax we render it (matching the
      typed experience); otherwise we bail to the default paste so internal
      rich copy/paste and image paste keep working untouched.
    */
    function looksLikeMarkdown(text: string): boolean {
        return (
            /^#{1,6}\s/m.test(text) || // # heading
            /^\s*[-*+]\s+\S/m.test(text) || // - bullet
            /^\s*\d+\.\s+\S/m.test(text) || // 1. ordered
            /^\s*>\s/m.test(text) || // > quote
            /^\s*```/m.test(text) || // ``` fence
            /^\s*[-*]\s+\[[ xX]\]\s/m.test(text) || // - [ ] task
            /\[[^\]]+\]\([^)]+\)/.test(text) || // [text](url)
            /!\[[^\]]*\]\([^)]+\)/.test(text) || // ![alt](src)
            /\*\*[^*\n]+\*\*/.test(text) || // **bold**
            /~~[^~\n]+~~/.test(text) || // ~~strike~~
            /`[^`\n]+`/.test(text) || // `code`
            /(?:^|\s)[*_][^*_\s][^*_\n]*[*_](?:\s|$)/.test(text) // *italic* / _italic_
        );
    }

    // The notes folder, resolved once and cached across editor instances.
    // Images are stored relative to it (`attachments/x.png`) so notes stay
    // portable; the editor resolves that to a loadable asset URL for display.
    let resolvedNotesDir: string | null = null;

    /** Turn an image src into something the WebView can load. The node KEEPS its
     *  original (portable, relative) src for Markdown serialization; only the
     *  rendered <img> is rewritten. Mirrors notes/preview.ts. */
    function resolveImageSrc(src: string): string {
        if (!src) return src;
        if (/^(https?:|data:|blob:|asset:|tauri:)/i.test(src)) return src;
        try {
            if (/^file:/i.test(src)) return convertFileSrc(src.replace(/^file:\/+/i, ''));
            if (/^([a-zA-Z]:[\\/]|\\\\)/.test(src)) return convertFileSrc(src);
            if (resolvedNotesDir) {
                const sep = resolvedNotesDir.includes('\\') ? '\\' : '/';
                return convertFileSrc(`${resolvedNotesDir}${sep}${src}`);
            }
        } catch {
            // fall through to the raw src
        }
        return src;
    }

    function imageWidth(value: unknown): number | null {
        const width = Number(value);
        return Number.isFinite(width) && width >= 80 && width <= 10_000 ? Math.round(width) : null;
    }

    // Image node whose stored src stays the portable relative path, but whose
    // rendered <img> resolves to an asset URL so the image actually shows.
    const LocalImage = TiptapImage.extend({
        addAttributes() {
            return {
                ...this.parent?.(),
                width: {
                    default: null,
                    parseHTML: (element) => {
                        const explicit = element.getAttribute('data-notes-width') ?? element.getAttribute('width');
                        const marker = /^width=(\d+)$/.exec(element.getAttribute('title') ?? '');
                        return imageWidth(explicit ?? marker?.[1]);
                    },
                    renderHTML: (attributes) => {
                        const width = imageWidth(attributes.width);
                        return width ? { 'data-notes-width': width } : {};
                    },
                },
            };
        },
        renderHTML({ HTMLAttributes }) {
            const attrs: Record<string, unknown> = { ...HTMLAttributes };
            if (typeof attrs.src === 'string') attrs.src = resolveImageSrc(attrs.src);
            const width = imageWidth(attrs.width);
            delete attrs.width;
            if (width) attrs.style = `width: ${width}px`;
            if (/^width=\d+$/.test(String(attrs.title ?? ''))) delete attrs.title;
            return ['img', mergeAttributes(this.options.HTMLAttributes, attrs)];
        },
        addStorage() {
            return {
                markdown: {
                    serialize(state: any, node: any, parent: any, index: number) {
                        const width = imageWidth(node.attrs.width);
                        const title = width ? `width=${width}` : node.attrs.title;
                        const image = node.type.create({ ...node.attrs, title });
                        defaultMarkdownSerializer.nodes.image(state, image, parent, index);
                    },
                },
            };
        },
        addNodeView() {
            return ({ node, getPos, editor: editorInstance }) => {
                let currentNode = node;
                let resizing = false;
                let startX = 0;
                let startWidth = 0;
                let nextWidth = 0;

                const dom = document.createElement('span');
                dom.className = 'notes-image';
                const image = document.createElement('img');
                image.draggable = false;
                dom.append(image);

                const render = () => {
                    image.src = resolveImageSrc(String(currentNode.attrs.src ?? ''));
                    image.alt = String(currentNode.attrs.alt ?? '');
                    image.style.width = imageWidth(currentNode.attrs.width)
                        ? `${imageWidth(currentNode.attrs.width)}px`
                        : '';
                };

                const finishResize = () => {
                    if (!resizing) return;
                    resizing = false;
                    window.removeEventListener('pointermove', resize);
                    window.removeEventListener('pointerup', finishResize);
                    try {
                        const pos = getPos();
                        if (typeof pos === 'number') {
                            editorInstance.view.dispatch(
                                editorInstance.state.tr.setNodeMarkup(pos, undefined, {
                                    ...currentNode.attrs,
                                    width: nextWidth,
                                }),
                            );
                        }
                    } catch {
                        // The image can disappear while it is being resized.
                    }
                };

                const resize = (event: PointerEvent) => {
                    if (!resizing) return;
                    const maxWidth = Math.max(160, editorInstance.view.dom.clientWidth - 48);
                    nextWidth = Math.round(Math.min(maxWidth, Math.max(160, startWidth + event.clientX - startX)));
                    image.style.width = `${nextWidth}px`;
                };

                if (editorInstance.isEditable) {
                    const handle = document.createElement('button');
                    handle.type = 'button';
                    handle.className = 'notes-image-resize';
                    handle.title = 'Drag to resize image';
                    handle.setAttribute('aria-label', 'Resize image');
                    handle.addEventListener('pointerdown', (event) => {
                        if (event.button !== 0) return;
                        event.preventDefault();
                        event.stopPropagation();
                        resizing = true;
                        startX = event.clientX;
                        startWidth = image.getBoundingClientRect().width || 160;
                        nextWidth = Math.round(startWidth);
                        window.addEventListener('pointermove', resize);
                        window.addEventListener('pointerup', finishResize, { once: true });
                    });
                    dom.append(handle);
                }

                render();
                return {
                    dom,
                    update(updatedNode) {
                        if (updatedNode.type !== currentNode.type) return false;
                        currentNode = updatedNode;
                        if (!resizing) render();
                        return true;
                    },
                    stopEvent: (event) => resizing || event.target instanceof HTMLButtonElement,
                    destroy() {
                        window.removeEventListener('pointermove', resize);
                        window.removeEventListener('pointerup', finishResize);
                    },
                };
            };
        },
    });

    // Editor extensions — shared with generateJSON() when healing escaped-HTML
    // notes, so both build from the exact same schema.
    const extensions = [
        StarterKit,
        Placeholder.configure({ placeholder: 'Start writing…  Markdown works.' }),
        TaskList,
        TaskItem.configure({ nested: true }),
        // GFM tables — tiptap-markdown round-trips these to/from Markdown once the
        // schema has the nodes, so table-heavy notes render instead of showing raw.
        Table.configure({ resizable: false }),
        TableRow,
        TableHeader,
        TableCell,
        // Live links. openOnClick:false so plain clicks edit; Ctrl/Cmd+click
        // opens in the system browser (handler in onMount) — never navigates
        // the webview. tiptap-markdown serializes the link mark back to Markdown.
        Link.configure({
            openOnClick: false,
            autolink: true,
            linkOnPaste: true,
            HTMLAttributes: { rel: 'noopener noreferrer nofollow' },
        }),
        // Local images: stored as a relative `attachments/x.png` path (portable),
        // rendered through convertFileSrc so the WebView can show them.
        LocalImage,
        // Standard Markdown blockquotes become callouts when they start with
        // [!NOTE], [!TIP], [!WARNING], or [!FILE]. The stored file stays plain
        // Markdown, so it remains useful outside KeepItLocal.
        CalloutBlocks,
        Collapsible,
        // [[Note Title]] links. Serializes back to literal [[...]] so the .ki
        // stays portable Markdown any other tool can read.
        //
        // Both callbacks read the prop INSIDE the closure, not at build time:
        // the editor is constructed once, so capturing the prop here would
        // freeze it. Returning null when there's no resolver means "unknown"
        // — see WikiLink.ts for why that isn't the same as "broken".
        WikiLink.configure({
            resolve: (title) => (resolveNote ? resolveNote(title) : null),
        }),
        // Same lazy read. With no host titles the query finds nothing and the
        // popup says so, rather than the trigger silently doing nothing.
        WikiLinkSuggestion.configure({ getTitles: () => noteTitles?.() ?? [] }),
        SlashCommandSuggestion.configure({
            getCommands: () => slashCommands.filter((command) => table || command.title !== 'Table'),
        }),
        // html:false keeps the .ki body clean Markdown on serialize; we drive
        // paste ourselves (transformPastedText off) so it never escapes tags.
        Markdown.configure({ html: false, linkify: true, transformPastedText: false }),
    ];

    /** Follow a link on Ctrl/Cmd+click, opening it in the system browser — never
     *  navigating the Tauri webview. A plain click just places the cursor (edit).
     *
     *  Wikilinks are handled first and separately: they're internal navigation,
     *  and a PLAIN click opens them (they're chips, not text you'd want to put a
     *  cursor inside). Ctrl/Cmd stays reserved for the external-URL path above,
     *  so the two can never fight over the same click. */
    function onEditorClick(event: MouseEvent) {
        hideBlockControls();
        const chip = (event.target as HTMLElement | null)?.closest('[data-wikilink]');
        if (chip) {
            // Ctrl/Cmd+click on a chip = let the editor select it (the escape
            // hatch for editing/deleting a link), not navigate.
            if (event.ctrlKey || event.metaKey) return;
            const title = chip.getAttribute('data-title');
            if (!title || !onNavigateNote) return;
            event.preventDefault();
            onNavigateNote(title);
            return;
        }
        const target = event.target as HTMLElement | null;
        const attachment = target?.closest('blockquote.notes-callout-file a[href^="attachments/"]');
        if (attachment) {
            if (event.ctrlKey || event.metaKey) return;
            const href = attachment.getAttribute('href');
            if (!href) return;
            event.preventDefault();
            void openAttachment(href, event.shiftKey);
            return;
        }
        if (!(event.ctrlKey || event.metaKey)) return;
        const href = target?.closest('a')?.getAttribute('href');
        if (!href) return;
        event.preventDefault();
        void openUrl(href).catch(() => {});
    }

    type BlockControl = { element: HTMLElement; top: number; left: number };
    let blockControl = $state<BlockControl | null>(null);

    function hideBlockControls(): void {
        blockControl = null;
    }

    function blockRange(element: HTMLElement): { from: number; to: number } | null {
        if (!editor) return null;
        try {
            const pos = editor.view.posAtDOM(element, 0);
            const resolvedPos = editor.state.doc.resolve(pos);
            for (let depth = resolvedPos.depth; depth > 0; depth--) {
                const node = resolvedPos.node(depth);
                if (node.isBlock) return { from: resolvedPos.before(depth), to: resolvedPos.after(depth) };
            }
        } catch {
            // A DOM mutation between pointer events can make posAtDOM fail.
        }
        return null;
    }

    function onEditorContextMenu(event: MouseEvent): void {
        if (!editable || !editor) return;
        const target = event.target as HTMLElement | null;
        const block = target?.closest<HTMLElement>('p, h1, h2, h3, h4, h5, h6, blockquote, pre, table, figure, details');
        if (!block || !editor.view.dom.contains(block)) return;
        const range = blockRange(block);
        if (!range) return;
        event.preventDefault();
        blockControl = {
            element: block,
            top: Math.round(Math.min(window.innerHeight - 34, Math.max(6, event.clientY + 6))),
            left: Math.round(Math.min(window.innerWidth - 58, Math.max(6, event.clientX + 6))),
        };
    }

    function addBlockBelow(): void {
        const control = blockControl;
        if (!control || !editor) return;
        const range = editor.view.dom.contains(control.element) ? blockRange(control.element) : null;
        if (!range) {
            hideBlockControls();
            return;
        }
        editor
            .chain()
            .focus()
            .insertContentAt(range.to, { type: 'paragraph', content: [{ type: 'text', text: '/' }] })
            .run();
        hideBlockControls();
    }

    function deleteHoveredBlock(): void {
        const control = blockControl;
        if (!control || !editor) return;
        const range = editor.view.dom.contains(control.element) ? blockRange(control.element) : null;
        if (!range) {
            hideBlockControls();
            return;
        }
        editor.view.dispatch(editor.state.tr.delete(range.from, range.to).scrollIntoView());
        editor.commands.focus();
        hideBlockControls();
    }

    function markdownForSave(editorInstance: Editor): string {
        return editorInstance.storage.markdown
            .getMarkdown()
            .replace(/^(\s*>\s*)\\\[!(NOTE|TIP|WARNING|FILE)\\\]/gim, '$1[!$2]');
    }

    onMount(async () => {
        // Resolve (once, cached) the notes folder so a relative image src can be
        // turned into a loadable asset URL by resolveImageSrc.
        if (resolvedNotesDir === null) {
            try {
                resolvedNotesDir = await invoke<string>('get_notes_dir');
            } catch {
                resolvedNotesDir = '';
            }
        }
        /*
          Heal notes corrupted by the earlier paste bug, whose body is
          entity-encoded HTML (`&lt;p&gt;…`): decode ONCE → real HTML →
          generateJSON parses it through the schema (sanitizing to known
          nodes/marks) → TipTap JSON. Passing JSON (not a string) means
          tiptap-markdown won't re-parse it as Markdown. Normal notes load
          their Markdown body unchanged via `content: value`.
        */
        // Three load paths:
        //  • escaped-HTML body → decode once → generateJSON (HTML → sanitized JSON)
        //  • whole body is one ```markdown fence → unwrap to its inner Markdown
        //  • normal Markdown → load as-is
        // The latter two stay Markdown strings for tiptap-markdown to parse.
        const escaped = looksLikeEscapedHtml(value);
        // Markdown path: unwrap an outer fence, then run smart-import cleanup
        // (Google-Docs/Word export artifacts → clean Markdown).
        const unwrapped = escaped ? value : sanitizeImportedMarkdown(stripOuterMarkdownFence(value));
        const healed = escaped || unwrapped !== value;
        const content = escaped ? generateJSON(decodeHtmlEntitiesOnce(value), extensions) : unwrapped;

        editor = new Editor({
            element,
            extensions,
            editorProps: {
                /*
                  Paste pipeline (never insert generated HTML as a string):
                  • Outer ```markdown fence (AI/chat output) → unwrap to its inner
                    Markdown so it renders instead of becoming a code block.
                  • Plain-text that looks like Markdown → parse it via tiptap-markdown
                    (even when an HTML flavour is present — chat apps often ship the
                    Markdown wrapped in `<pre>` HTML we don't want).
                  • Everything else (rich web/Word HTML, plain text, images) → bail
                    to ProseMirror's default paste, which parses through the schema
                    (sanitized into known nodes).
                */
                handlePaste: (_view, event) => {
                    const cd = event.clipboardData;
                    if (!cd) return false;
                    const text = sanitizeImportedMarkdown(
                        stripOuterMarkdownFence(cd.getData('text/plain') ?? ''),
                    ).trim();
                    if (text && looksLikeMarkdown(text)) {
                        editor?.chain().focus().insertContent(text).run();
                        return true;
                    }
                    return false;
                },
            },
            content,
            autofocus: autofocus ? 'end' : false,
            editable,
            onUpdate: ({ editor }) => {
                if (editable) onChange?.(markdownForSave(editor));
            },
            onTransaction: () => {
                tick += 1;
            },
        });

        // Open links in the system browser on Ctrl/Cmd+click (never navigate).
        editor.view.dom.addEventListener('click', onEditorClick);
        if (editable) {
            editor.view.dom.addEventListener('contextmenu', onEditorContextMenu);
            element.addEventListener('scroll', hideBlockControls);
        }

        // A healed note (escaped-HTML decoded, or an outer ```markdown fence
        // unwrapped) is normalized in memory only — persist the clean Markdown
        // back so the .ki file itself is repaired. Guarded so untouched notes
        // never trigger a write.
        if (healed && editor && editable) {
            onChange?.(markdownForSave(editor));
        }
    });

    onDestroy(() => {
        editor?.view?.dom?.removeEventListener('click', onEditorClick);
        editor?.view?.dom?.removeEventListener('contextmenu', onEditorContextMenu);
        element?.removeEventListener('scroll', hideBlockControls);
        editor?.destroy();
        editor = null;
    });

    // Read `tick` so these recompute as the selection/content changes.
    function active(name: string, attrs?: Record<string, unknown>): boolean {
        void tick;
        return editor?.isActive(name, attrs) ?? false;
    }

    /** Heading level of the caret's block: 1, 2, or 0 (paragraph/other). */
    function headingLevel(): number {
        if (active('heading', { level: 1 })) return 1;
        if (active('heading', { level: 2 })) return 2;
        return 0;
    }

    /** One button cycles the caret's block: none → H1 → H2 → none. */
    function cycleHeading() {
        const chain = editor?.chain().focus();
        if (!chain) return;
        const level = headingLevel();
        if (level === 0) chain.setHeading({ level: 1 }).run();
        else if (level === 1) chain.setHeading({ level: 2 }).run();
        else chain.setParagraph().run();
    }

    /** Pick a local image, copy it into the notes folder (portable) and insert
     *  it. The stored src is the relative `attachments/x.png`; display resolves
     *  via convertFileSrc. */
    async function insertImage() {
        const selected = await openFileDialog({
            multiple: false,
            filters: [
                { name: 'Image', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg'] },
            ],
        });
        if (typeof selected !== 'string') return;
        try {
            const rel = await invoke<string>('copy_note_asset', { sourcePath: selected });
            editor?.chain().focus().insertContent({ type: 'image', attrs: { src: rel } }).run();
        } catch (e) {
            toast(`Couldn't insert image: ${e}`, 'error');
        }
    }

    type NoteAttachment = { path: string; name: string; bytes: number };

    function formatBytes(bytes: number): string {
        if (bytes < 1024) return `${bytes} B`;
        const units = ['KB', 'MB', 'GB'];
        let value = bytes / 1024;
        let unit = 0;
        while (value >= 1024 && unit < units.length - 1) {
            value /= 1024;
            unit++;
        }
        return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
    }

    async function insertFile() {
        const selected = await openFileDialog({ title: 'Attach a file', multiple: false });
        if (typeof selected !== 'string') return;
        try {
            const rel = await invoke<string>('copy_note_asset', { sourcePath: selected });
            const attachment = await invoke<NoteAttachment>('get_note_attachment', { path: rel });
            const label = attachment.name.replace(/[\\[\]]/g, '\\$&');
            const card = `> [!FILE] [${label}](${rel})\n> ${formatBytes(attachment.bytes)} · Click to open · Shift-click to reveal`;
            editor?.chain().focus().insertContent(card).run();
        } catch (error) {
            toast(`Couldn't attach file: ${error}`, 'error');
        }
    }

    async function openAttachment(path: string, reveal: boolean): Promise<void> {
        try {
            const attachment = await invoke<NoteAttachment>('get_note_attachment', { path });
            if (reveal) await revealItemInDir(attachment.path);
            else await openPath(attachment.path);
        } catch (error) {
            toast(`Couldn't open attachment: ${error}`, 'error');
        }
    }

    type CalloutKind = 'NOTE' | 'TIP' | 'WARNING';

    function calloutBlock(kind: CalloutKind) {
        return {
            type: 'blockquote',
            content: [{ type: 'paragraph', content: [{ type: 'text', text: `[!${kind}] ` }] }],
        };
    }

    function insertCallout(kind: CalloutKind = 'NOTE'): void {
        editor?.chain().focus().insertContent(calloutBlock(kind)).run();
    }

    function insertCollapsible(): void {
        editor
            ?.chain()
            .focus()
            .insertContent({
                type: 'collapsible',
                attrs: { title: 'Details', open: true },
                content: [{ type: 'paragraph' }],
            })
            .run();
    }

    function collapseTitle(): string {
        void tick;
        return String(editor?.getAttributes('collapsible').title ?? 'Details');
    }

    function setCollapseTitle(title: string): void {
        editor?.chain().focus().updateAttributes('collapsible', { title }).run();
    }

    function replaceSlashRange(editorInstance: Editor, range: { from: number; to: number }) {
        return editorInstance.chain().focus().deleteRange(range);
    }

    const slashCommands: SlashCommand[] = [
        {
            title: 'Heading 1',
            detail: 'Large section heading',
            keywords: ['title'],
            run: (editorInstance, range) => replaceSlashRange(editorInstance, range).setHeading({ level: 1 }).run(),
        },
        {
            title: 'Heading 2',
            detail: 'Subsection heading',
            keywords: ['subtitle'],
            run: (editorInstance, range) => replaceSlashRange(editorInstance, range).setHeading({ level: 2 }).run(),
        },
        {
            title: 'Checklist',
            detail: 'Track an action',
            keywords: ['task', 'todo'],
            run: (editorInstance, range) => replaceSlashRange(editorInstance, range).toggleTaskList().run(),
        },
        {
            title: 'Note callout',
            detail: 'Important context',
            keywords: ['callout', 'note'],
            run: (editorInstance, range) => replaceSlashRange(editorInstance, range).insertContent(calloutBlock('NOTE')).run(),
        },
        {
            title: 'Tip callout',
            detail: 'Helpful guidance',
            keywords: ['callout', 'tip'],
            run: (editorInstance, range) => replaceSlashRange(editorInstance, range).insertContent(calloutBlock('TIP')).run(),
        },
        {
            title: 'Warning callout',
            detail: 'Needs attention',
            keywords: ['callout', 'warning'],
            run: (editorInstance, range) => replaceSlashRange(editorInstance, range).insertContent(calloutBlock('WARNING')).run(),
        },
        {
            title: 'Collapsible section',
            detail: 'Hide supporting detail',
            keywords: ['details', 'toggle'],
            run: (editorInstance, range) => {
                replaceSlashRange(editorInstance, range)
                    .insertContent({
                        type: 'collapsible',
                        attrs: { title: 'Details', open: true },
                        content: [{ type: 'paragraph' }],
                    })
                    .run();
            },
        },
        {
            title: 'File',
            detail: 'Attach a local file',
            keywords: ['attachment', 'document'],
            run: (editorInstance, range) => {
                replaceSlashRange(editorInstance, range).run();
                void insertFile();
            },
        },
        {
            title: 'Image',
            detail: 'Embed a local image',
            keywords: ['photo'],
            run: (editorInstance, range) => {
                replaceSlashRange(editorInstance, range).run();
                void insertImage();
            },
        },
        {
            title: 'Table',
            detail: 'Three columns and rows',
            keywords: ['grid'],
            run: (editorInstance, range) => {
                replaceSlashRange(editorInstance, range)
                    .insertTable({ rows: 3, cols: 3, withHeaderRow: true })
                    .run();
            },
        },
        {
            title: 'Divider',
            detail: 'Separate sections',
            keywords: ['rule'],
            run: (editorInstance, range) => replaceSlashRange(editorInstance, range).setHorizontalRule().run(),
        },
    ];

    const TOOLS = [
        { icon: Bold, title: 'Bold (Ctrl+B)', is: () => active('bold'), run: () => editor?.chain().focus().toggleBold().run() },
        { icon: Italic, title: 'Italic (Ctrl+I)', is: () => active('italic'), run: () => editor?.chain().focus().toggleItalic().run() },
        { icon: Strikethrough, title: 'Strikethrough', is: () => active('strike'), run: () => editor?.chain().focus().toggleStrike().run() },
        { icon: Heading, title: 'Heading — click to cycle H1 → H2 → none', is: () => headingLevel() > 0, run: cycleHeading },
        { icon: List, title: 'Bullet list', is: () => active('bulletList'), run: () => editor?.chain().focus().toggleBulletList().run() },
        { icon: ListOrdered, title: 'Numbered list', is: () => active('orderedList'), run: () => editor?.chain().focus().toggleOrderedList().run() },
        { icon: ListChecks, title: 'Checklist', is: () => active('taskList'), run: () => editor?.chain().focus().toggleTaskList().run() },
        { icon: Quote, title: 'Quote', is: () => active('blockquote'), run: () => editor?.chain().focus().toggleBlockquote().run() },
        { icon: Code, title: 'Code block', is: () => active('codeBlock'), run: () => editor?.chain().focus().toggleCodeBlock().run() },
        { icon: TableIcon, title: 'Insert table', is: () => active('table'), run: () => editor?.chain().focus().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run() },
        { icon: ImageIcon, title: 'Insert image', is: () => active('image'), run: insertImage },
        { icon: Paperclip, title: 'Attach file', is: () => false, run: insertFile },
        { icon: MessageSquare, title: 'Insert note callout', is: () => active('blockquote'), run: () => insertCallout() },
        { icon: Lightbulb, title: 'Insert tip callout', is: () => false, run: () => insertCallout('TIP') },
        { icon: TriangleAlert, title: 'Insert warning callout', is: () => false, run: () => insertCallout('WARNING') },
        { icon: ChevronsUpDown, title: 'Insert collapsible section', is: () => active('collapsible'), run: insertCollapsible },
    ];
</script>

<div class="ne" class:is-readonly={!editable}>
    {#if showToolbar}
        <div class="ne-toolbar" role="toolbar" aria-label="Formatting">
            {#each table ? TOOLS : TOOLS.filter((t) => t.title !== 'Insert table') as tool (tool.title)}
                {@const Icon = tool.icon}
                <button
                    type="button"
                    class="ne-tool"
                    class:is-active={tool.is()}
                    title={tool.title}
                    aria-label={tool.title}
                    aria-pressed={tool.is()}
                    onclick={tool.run}
                >
                    <Icon class="ne-tool-ico" />
                </button>
            {/each}
            {#if active('collapsible')}
                <input
                    class="ne-collapse-title"
                    value={collapseTitle()}
                    aria-label="Collapsible section title"
                    oninput={(event) => setCollapseTitle(event.currentTarget.value)}
                />
            {/if}
        </div>
    {/if}
    <div class="ne-surface" bind:this={element}></div>
    {#if editable && blockControl}
        <div
            class="ne-block-controls"
            role="group"
            aria-label="Block controls"
            style={`top: ${blockControl.top}px; left: ${blockControl.left}px`}
        >
            <button type="button" title="Add block below" aria-label="Add block below" onclick={addBlockBelow}>
                <Plus />
            </button>
            <button type="button" title="Delete block" aria-label="Delete block" onclick={deleteHoveredBlock}>
                <Trash2 />
            </button>
        </div>
    {/if}
</div>

<style>
    .ne {
        display: flex;
        flex-direction: column;
        height: 100%;
        min-height: 0;
    }
    .ne-toolbar {
        flex: none;
        display: flex;
        align-items: center;
        gap: 2px;
        padding: 6px 8px;
        border-bottom: 1px solid var(--color-border);
        flex-wrap: wrap;
    }
    .ne-tool {
        display: grid;
        place-items: center;
        width: 30px;
        height: 30px;
        border: none;
        border-radius: 7px;
        background: transparent;
        color: var(--color-text-secondary);
        cursor: pointer;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .ne-tool:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .ne-tool.is-active {
        background: color-mix(in srgb, var(--color-accent) 16%, transparent);
        color: var(--color-accent);
    }
    .ne-collapse-title {
        width: min(180px, 28vw);
        height: 28px;
        margin-left: 5px;
        padding: 0 8px;
        border: 1px solid var(--color-border);
        border-radius: 6px;
        background: transparent;
        color: var(--color-text-secondary);
        font: inherit;
        font-size: 11.5px;
    }
    .ne-collapse-title:focus {
        outline: 2px solid color-mix(in srgb, var(--color-accent) 55%, transparent);
        outline-offset: 1px;
    }
    .ne :global(.ne-tool-ico) {
        width: 15px;
        height: 15px;
    }

    .ne-surface {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
    }
    /* TipTap renders a `.tiptap.ProseMirror` element inside .ne-surface. */
    .ne-surface :global(.tiptap) {
        min-height: 100%;
        padding: 20px 24px 64px;
        outline: none;
        color: var(--color-text);
        font-size: 15px;
        line-height: 1.65;
    }
    .ne-surface :global(.tiptap > * + *) {
        margin-top: 0.75em;
    }
    .ne-surface :global(.tiptap h1) {
        font-size: 1.6em;
        font-weight: 700;
        line-height: 1.2;
        margin-top: 1.2em;
    }
    .ne-surface :global(.tiptap h2) {
        font-size: 1.3em;
        font-weight: 700;
        line-height: 1.25;
        margin-top: 1.1em;
    }
    .ne-surface :global(.tiptap h3) {
        font-size: 1.12em;
        font-weight: 600;
        margin-top: 1em;
    }
    .ne-surface :global(.tiptap ul),
    .ne-surface :global(.tiptap ol) {
        padding-left: 1.4em;
    }
    .ne-surface :global(.tiptap ul) {
        list-style: disc;
    }
    .ne-surface :global(.tiptap ol) {
        list-style: decimal;
    }
    .ne-surface :global(.tiptap blockquote) {
        border-left: 3px solid color-mix(in srgb, var(--color-accent) 50%, var(--color-border));
        padding-left: 14px;
        color: var(--color-text-secondary);
    }
    .ne-surface :global(.tiptap blockquote.notes-callout) {
        position: relative;
        padding: 9px 12px 9px 15px;
        border-left-color: color-mix(in srgb, var(--color-accent) 70%, var(--color-border));
        color: var(--color-text);
    }
    .ne-surface :global(.tiptap blockquote.notes-callout::before) {
        display: block;
        margin-bottom: 3px;
        color: var(--color-accent);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.08em;
        text-transform: uppercase;
        content: 'Note';
    }
    .ne-surface :global(.tiptap blockquote.notes-callout-tip) {
        border-left-color: color-mix(in srgb, var(--color-success, var(--color-accent)) 70%, var(--color-border));
    }
    .ne-surface :global(.tiptap blockquote.notes-callout-tip::before) {
        color: var(--color-success, var(--color-accent));
        content: 'Tip';
    }
    .ne-surface :global(.tiptap blockquote.notes-callout-warning) {
        border-left-color: color-mix(in srgb, var(--color-warning, var(--color-accent)) 70%, var(--color-border));
    }
    .ne-surface :global(.tiptap blockquote.notes-callout-warning::before) {
        color: var(--color-warning, var(--color-accent));
        content: 'Attention';
    }
    .ne-surface :global(.tiptap blockquote.notes-callout-file) {
        border-left-color: color-mix(in srgb, var(--color-text) 24%, var(--color-border));
    }
    .ne-surface :global(.tiptap blockquote.notes-callout-file::before) {
        color: var(--color-text-secondary);
        content: 'Attachment';
    }
    .ne-surface :global(.tiptap .notes-callout-marker) {
        display: none;
    }
    .ne-surface :global(.tiptap blockquote.notes-callout-file a) {
        color: var(--color-text);
        font-weight: 650;
        text-decoration: none;
    }
    .ne-surface :global(.tiptap blockquote.notes-callout-file a:hover) {
        color: var(--color-accent);
        text-decoration: underline;
    }
    .ne-surface :global(.tiptap details.notes-collapsible) {
        margin: 0.75em 0;
        padding: 0 0 0 14px;
        border-left: 2px solid var(--color-border);
    }
    .ne-surface :global(.tiptap details.notes-collapsible > summary) {
        padding: 4px 0;
        color: var(--color-text);
        font-weight: 650;
        cursor: pointer;
        list-style-position: outside;
    }
    .ne-surface :global(.tiptap details.notes-collapsible > summary::marker) {
        color: var(--color-muted);
    }
    .ne-surface :global(.tiptap .notes-collapsible-content) {
        padding-bottom: 8px;
        color: var(--color-text-secondary);
    }
    .ne-surface :global(.tiptap .notes-collapsible-content > * + *) {
        margin-top: 0.65em;
    }
    .ne-surface :global(.tiptap code) {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 0.88em;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 4px;
        padding: 1px 5px;
    }
    .ne-surface :global(.tiptap pre) {
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 10px;
        padding: 12px 14px;
        overflow-x: auto;
    }
    .ne-surface :global(.tiptap pre code) {
        background: none;
        border: none;
        padding: 0;
    }
    .ne-surface :global(.tiptap a) {
        color: var(--color-accent);
        text-decoration: underline;
        cursor: pointer;
    }
    /* Wikilink chip. Deliberately NOT styled like an <a>: an external link
       leaves the app, an internal one doesn't, and they shouldn't look alike.
       Bracket glyphs are drawn in ::before/::after so the underlying Markdown
       stays visible as syntax without living in the document text. */
    .ne-surface :global(.tiptap .wikilink) {
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 8%, transparent);
        border-radius: 4px;
        padding: 0 2px;
        cursor: pointer;
        text-decoration: none;
    }
    .ne-surface :global(.tiptap .wikilink::before) {
        content: '[[';
        opacity: 0.4;
    }
    .ne-surface :global(.tiptap .wikilink::after) {
        content: ']]';
        opacity: 0.4;
    }
    .ne-surface :global(.tiptap .wikilink:hover) {
        background: color-mix(in srgb, var(--color-accent) 16%, transparent);
    }
    /* Unresolved: points at a note that doesn't exist (yet). Muted + dashed
       rather than red — a link you haven't written yet is a normal state in
       note-taking, not an error. */
    .ne-surface :global(.tiptap .wikilink.is-unresolved) {
        color: var(--color-muted);
        background: transparent;
        border-bottom: 1px dashed var(--color-border);
        border-radius: 0;
    }
    .ne-surface :global(.tiptap .wikilink.is-unresolved:hover) {
        background: color-mix(in srgb, var(--color-muted) 10%, transparent);
    }
    /* ProseMirror's selected-atom state — without this, clicking a chip with
       Ctrl gives no visual feedback that it's selected. */
    .ne-surface :global(.tiptap .wikilink.ProseMirror-selectednode) {
        outline: 2px solid color-mix(in srgb, var(--color-accent) 50%, transparent);
    }
    .ne-surface :global(.tiptap img) {
        max-width: 100%;
        height: auto;
        border-radius: 8px;
        margin: 0.25em 0;
    }
    .ne-surface :global(.tiptap .notes-image) {
        position: relative;
        display: inline-block;
        max-width: 100%;
        line-height: 0;
    }
    .ne-surface :global(.tiptap .notes-image img) {
        display: block;
        margin: 0;
    }
    .ne-surface :global(.tiptap .notes-image-resize) {
        position: absolute;
        right: -5px;
        bottom: -5px;
        width: 12px;
        height: 12px;
        padding: 0;
        border: 2px solid var(--color-bg);
        border-radius: 50%;
        background: var(--color-accent);
        box-shadow: 0 1px 4px rgb(0 0 0 / 0.28);
        cursor: nwse-resize;
        opacity: 0;
        transition: opacity var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .ne-surface :global(.tiptap .notes-image:hover .notes-image-resize),
    .ne-surface :global(.tiptap .notes-image:focus-within .notes-image-resize) {
        opacity: 1;
    }
    /* GFM tables. */
    .ne-surface :global(.tiptap table) {
        border-collapse: collapse;
        width: 100%;
        margin: 0.25em 0;
        font-size: 0.92em;
        table-layout: fixed;
    }
    .ne-surface :global(.tiptap th),
    .ne-surface :global(.tiptap td) {
        border: 1px solid var(--color-border);
        padding: 6px 10px;
        text-align: left;
        vertical-align: top;
        word-break: break-word;
    }
    .ne-surface :global(.tiptap th) {
        background: var(--color-panel-2);
        font-weight: 600;
    }
    .ne-surface :global(.tiptap table p) {
        margin: 0;
    }
    /* Checklist (TaskList/TaskItem). */
    .ne-surface :global(.tiptap ul[data-type='taskList']) {
        list-style: none;
        padding-left: 0.2em;
    }
    .ne-surface :global(.tiptap ul[data-type='taskList'] li) {
        display: flex;
        align-items: flex-start;
        gap: 8px;
    }
    .ne-surface :global(.tiptap ul[data-type='taskList'] li > label) {
        margin-top: 0.3em;
    }
    /* Placeholder (empty doc). */
    .ne-surface :global(.tiptap p.is-editor-empty:first-child::before) {
        content: attr(data-placeholder);
        float: left;
        height: 0;
        pointer-events: none;
        color: var(--color-muted);
    }

    .ne-block-controls {
        position: fixed;
        z-index: 61;
        display: flex;
        gap: 1px;
        padding: 2px;
        border: 1px solid var(--color-border);
        border-radius: 7px;
        background: var(--color-bg);
        box-shadow: var(--shadow-sm, 0 4px 12px rgb(0 0 0 / 0.16));
    }
    .ne-block-controls button {
        display: grid;
        place-items: center;
        width: 25px;
        height: 25px;
        padding: 0;
        border: none;
        border-radius: 5px;
        background: transparent;
        color: var(--color-muted);
        cursor: pointer;
    }
    .ne-block-controls button:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .ne-block-controls button:last-child:hover {
        color: var(--color-error);
    }
    .ne-block-controls :global(svg) {
        width: 13px;
        height: 13px;
    }

    /* ─── [[ ]] autocomplete popup ────────────────────────────────────
       Unscoped on purpose: WikiLinkSuggestion appends this to document.body
       (Suggestion positions it against the caret in viewport coords, so it
       can't live inside the editor's overflow). Kept here rather than in
       styles.css so it sits beside the code that builds it — NotesEditor is
       its only owner. */
    :global(.wikilink-menu) {
        position: fixed;
        z-index: 60;
        min-width: 200px;
        max-width: 320px;
        max-height: 240px;
        overflow-y: auto;
        padding: 4px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 10px;
        box-shadow: var(--shadow-md, 0 8px 24px rgb(0 0 0 / 0.28));
    }
    :global(.wikilink-opt) {
        position: relative;
        display: block;
        width: 100%;
        padding: 6px 10px 6px 14px;
        border: none;
        border-radius: 6px;
        background: none;
        color: var(--color-text-secondary);
        font-size: 12.5px;
        text-align: left;
        cursor: pointer;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    /* Canonical selected-item pattern: neutral panel-2 + accent pill strip +
       accent text at regular weight. Not an accent-tinted row. */
    :global(.wikilink-opt.is-active) {
        background: var(--color-panel-2);
        color: var(--color-accent);
    }
    :global(.wikilink-opt.is-active::before) {
        content: '';
        position: absolute;
        left: 3px;
        top: 6px;
        bottom: 6px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    :global(.wikilink-menu-empty) {
        padding: 6px 10px;
        color: var(--color-muted);
        font-size: 12px;
    }
    :global(.notes-slash-menu) {
        position: fixed;
        z-index: 60;
        width: min(272px, calc(100vw - 16px));
        max-height: 272px;
        overflow-y: auto;
        padding: 4px;
        border: 1px solid var(--color-border);
        border-radius: 10px;
        background: var(--color-panel);
        box-shadow: var(--shadow-md, 0 8px 24px rgb(0 0 0 / 0.28));
    }
    :global(.notes-slash-option) {
        display: grid;
        width: 100%;
        gap: 1px;
        padding: 7px 9px;
        border: none;
        border-radius: 7px;
        background: transparent;
        color: var(--color-text-secondary);
        font: inherit;
        text-align: left;
        cursor: pointer;
    }
    :global(.notes-slash-option:hover),
    :global(.notes-slash-option.is-active) {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    :global(.notes-slash-option-title) {
        font-size: 12px;
        font-weight: 650;
    }
    :global(.notes-slash-option-detail),
    :global(.notes-slash-empty) {
        color: var(--color-muted);
        font-size: 10.5px;
    }
    :global(.notes-slash-empty) {
        padding: 8px 9px;
    }
</style>
