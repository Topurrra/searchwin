<script lang="ts">
    /*
      Notes — local, open-format note-taking. Two panes: a list on the left
      (search · pinned-first · per-row pin/delete) and the TipTap WYSIWYG editor
      on the right. Notes are `.ki` files (Markdown + YAML frontmatter) in
      Documents/KeepItLocal Notes — see stores/notes.ts.

      The editor is keyed on the note path so each note gets a fresh TipTap
      instance (no mid-typing resets). Edits autosave (debounced) via the store.
    */
    import { onMount, tick } from 'svelte';
    import { get } from 'svelte/store';
    import {
        NotebookPen,
        Plus,
        Search,
        Pin,
        Trash2,
        FolderOpen,
        Folder,
        FolderPlus,
        Inbox,
        FileText,
        Download,
        Check,
        ChevronDown,
        Pencil,
        RotateCcw,
        Clock3,
        History,
        CalendarDays,
        Ellipsis,
        Tags,
        PanelRightOpen,
        RefreshCw,
        X,
        Columns2,
        Maximize2,
        Minimize2,
        Network,
        ClipboardPlus,
    } from '@lucide/svelte';
    import { marked } from 'marked';
    import { open as openFileDialog, save } from '@tauri-apps/plugin-dialog';
    import { readTextFile, writeTextFile } from '@tauri-apps/plugin-fs';
    import { invoke } from '@tauri-apps/api/core';
    import { escToClear } from '$lib/actions/escToClear';
    import { toast } from '$lib/stores/toasts';
    import { confirm } from '$lib/stores/confirmDialog';
    import NotesEditor from './NotesEditor.svelte';
    import NoteTagInput from './NoteTagInput.svelte';
    import { extractWikiLinks, type WikiLinkReference } from './tiptap/WikiLink';
    import {
        notes,
        notesReady,
        activeNote,
        saveStatus,
        externalNoteConflict,
        initNotesStore,
        openNote,
        reloadActiveNote,
        keepActiveNoteChanges,
        restoreActiveNoteRevision,
        newNote,
        removeNote,
        togglePin,
        setActiveBody,
        setActiveTitle,
        setActiveTags,
        allTags,
        noteTemplates,
        refreshNoteTemplates,
        newNoteFromTemplate,
        openNoteTemplatesFolder,
        flushSave,
        openNotesFolder,
        currentFolder,
        noteFolders,
        createNoteFolder,
        deleteNoteFolder,
        renameNoteFolder,
        listTrashedNotes,
        restoreTrashedNote,
        deleteTrashedNote,
        emptyNoteTrash,
        type NoteSummary,
        type TrashedNote,
        moveNote,
        captureNote,
        parseNote,
        INBOX_FOLDER,
        DAILY_FOLDER,
        isInboxFolder,
        localDateKey,
        openTodayNote,
        decodeHtmlEntitiesOnce,
        looksLikeEscapedHtml,
        sanitizeImportedMarkdown,
        stripOuterMarkdownFence,
        type NoteDoc,
        type NoteFile,
    } from '$lib/stores/notes';

    const NOTES_EXAMPLES_FOLDER = 'Notes examples';
    const NOTES_EXAMPLES = [
        {
            title: 'Notes example - Writing blocks',
            body: `# Writing blocks

This note shows the main writing blocks in one place.

> [!NOTE] Callouts keep important context close to the work.

> [!TIP] Use the toolbar or slash commands to add blocks while writing.

> [!WARNING] This is only example content. You can safely edit or delete this folder.

## Checklist

- [ ] Try adding a new task
- [x] Confirm completed tasks are visible

## Table

| Block | Try it |
| --- | --- |
| Callout | Add a note, tip, or warning |
| Collapsible | Click its title, then write inside it |

:::details Release checklist
- [ ] Expand and collapse this section
- [ ] Add a paragraph inside it
:::

## File cards

Use the paperclip to attach a real local file. Its card appears here after you add it.`,
        },
        {
            title: 'Notes example - Connected brief',
            body: `# Connected brief

[[Notes example - Writing blocks]]

This note exists to test wiki links, backlinks, the outline, and split view.

## Context

Open the linked writing-block note in split view, then return here without losing your place.

## Next step

[[Notes example - Revision sandbox]]

> [!TIP] Open the graph after creating these examples to see the three-note connection.`,
        },
        {
            title: 'Notes example - Revision sandbox',
            body: `# Revision sandbox

[[Notes example - Writing blocks]]
[[Notes example - Connected brief]]

Make a small edit here, wait for it to save, then open local history to compare or restore a revision.

## Capture and import

Use Capture to send text into Inbox, or import a Markdown file to review the import flow.`,
        },
    ] as const;

    let query = $state('');
    let templatesOpen = $state(false);
    let noteMenuOpen = $state(false);
    let noteMenuEl = $state<HTMLDivElement | null>(null);
    /** Container for the template split-button, so a click outside it can
     *  dismiss the menu without swallowing clicks inside it. */
    let tplEl = $state<HTMLDivElement | null>(null);

    type LibraryView = 'all' | 'inbox' | 'recent' | 'pinned' | 'folder' | 'trash';
    const initialFolder = get(currentFolder);
    let libraryView = $state<LibraryView>(
        initialFolder === INBOX_FOLDER ? 'inbox' : initialFolder === null ? 'all' : 'folder',
    );
    let tagFilter = $state<string | null>(null);
    let focusMode = $state(false);
    let splitNote = $state<NoteDoc | null>(null);
    let splitLoading = $state(false);
    let splitLoadSequence = 0;
    let splitWidth = $state(360);
    let splitDragging = $state(false);
    let workspaceEl = $state<HTMLDivElement | null>(null);
    let contextEl = $state<HTMLElement | null>(null);

    function maxSplitWidth(): number {
        const workspaceWidth = workspaceEl?.clientWidth ?? splitWidth;
        const contextWidth = contextEl?.getBoundingClientRect().width ?? 0;
        return Math.max(280, workspaceWidth - contextWidth - 360);
    }

    function setSplitWidth(width: number): void {
        splitWidth = Math.min(maxSplitWidth(), Math.max(280, Math.round(width)));
    }

    function onSplitDividerDown(event: PointerEvent): void {
        if (event.button !== 0) return;
        splitDragging = true;
        (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
        event.preventDefault();
    }

    function onSplitDividerMove(event: PointerEvent): void {
        if (!splitDragging || !workspaceEl) return;
        const rect = workspaceEl.getBoundingClientRect();
        const contextWidth = contextEl?.getBoundingClientRect().width ?? 0;
        setSplitWidth(rect.right - contextWidth - event.clientX);
    }

    function onSplitDividerUp(event: PointerEvent): void {
        if (!splitDragging) return;
        splitDragging = false;
        try {
            (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
        } catch {
            // Pointer capture can already be released by the browser.
        }
    }

    function onSplitDividerKeydown(event: KeyboardEvent): void {
        if (event.key === 'ArrowLeft') {
            setSplitWidth(splitWidth + 24);
        } else if (event.key === 'ArrowRight') {
            setSplitWidth(splitWidth - 24);
        } else {
            return;
        }
        event.preventDefault();
    }

    async function openSplitPreview(path: string): Promise<void> {
        const active = get(activeNote);
        if (active?.path === path && !(await flushSave())) return;
        const sequence = ++splitLoadSequence;
        splitLoading = true;
        try {
            const file = await invoke<NoteFile>('read_note', { path });
            if (sequence !== splitLoadSequence) return;
            const fallback = $notes.find((note) => note.path === path)?.title || 'Untitled';
            const parsed = parseNote(file.content, fallback);
            splitNote = { path, ...parsed };
        } catch (error) {
            if (sequence === splitLoadSequence) toast(`Could not open split note: ${String(error)}`, 'error');
        } finally {
            if (sequence === splitLoadSequence) splitLoading = false;
        }
    }

    function toggleSplit(): void {
        if (splitNote || splitLoading) {
            splitLoadSequence++;
            splitLoading = false;
            splitNote = null;
            return;
        }
        const path = get(activeNote)?.path;
        if (path) void openSplitPreview(path);
    }

    // The title field is decoupled from the autosave store via a local buffer.
    // Binding the input straight to $activeNote.meta.title would rewrite
    // input.value on every keystroke (setActiveTitle updates the store, which
    // feeds back into the binding) and fight the caret on fast typing / rapid
    // Delete+retype. We seed the buffer only when a DIFFERENT note becomes
    // active (path change) — never on our own per-keystroke edits — so typing
    // owns the input and the store update is just a side-effect for the save.
    let titleBuffer = $state('');
    let titleSeededPath = $state<string | null>(null);
    $effect(() => {
        const path = $activeNote?.path ?? null;
        if (path !== titleSeededPath) {
            titleSeededPath = path;
            titleBuffer = $activeNote?.meta.title ?? '';
        }
    });

    // ── Export as text ──────────────────────────────────────────────────────
    /** Convert the note's Markdown to clean, readable plain text — strips the
     *  syntax (#, **, `, >, links, …) but keeps block structure and list
     *  bullets. Renders via marked, then flattens the HTML block-by-block. */
    function markdownToText(md: string): string {
        // Heal any escaped-HTML body before flattening, so exports never leak entities/tags.
        const src = looksLikeEscapedHtml(md) ? decodeHtmlEntitiesOnce(md) : md;
        const html = marked.parse(src) as string;
        const doc = new DOMParser().parseFromString(html, 'text/html');
        const collapse = (el: Element | null) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();
        const out: string[] = [];
        for (const node of Array.from(doc.body.children)) {
            const tag = node.tagName.toLowerCase();
            if (/^h[1-6]$/.test(tag) || tag === 'p') {
                const t = collapse(node);
                if (t) out.push(t, '');
            } else if (tag === 'ul' || tag === 'ol') {
                let i = 1;
                for (const li of Array.from(node.children)) {
                    const prefix = tag === 'ol' ? `${i++}. ` : '• ';
                    out.push(prefix + collapse(li));
                }
                out.push('');
            } else if (tag === 'pre') {
                out.push((node.textContent ?? '').replace(/\n+$/, ''), '');
            } else if (tag === 'hr') {
                out.push('—'.repeat(20), '');
            } else if (tag === 'table') {
                for (const row of Array.from(node.querySelectorAll('tr'))) {
                    const cells = Array.from(row.querySelectorAll('th,td')).map((c) => collapse(c));
                    out.push(cells.join('  |  '));
                }
                out.push('');
            } else {
                const t = collapse(node);
                if (t) out.push(t, '');
            }
        }
        return out.join('\n').replace(/\n{3,}/g, '\n\n').trim() + '\n';
    }

    async function exportText() {
        const note = get(activeNote);
        if (!note) return;
        const path = await save({
            title: 'Export note as text',
            defaultPath: `${note.meta.title || 'note'}.txt`,
            filters: [{ name: 'Text', extensions: ['txt'] }],
        });
        if (!path) return;
        try {
            const body = markdownToText(note.body || '');
            const title = (note.meta.title || '').trim();
            // Prepend the title unless the body already opens with it as a heading.
            const hasH1 = (note.body || '').trimStart().startsWith('# ');
            const content = !hasH1 && title ? `${title}\n\n${body}` : body;
            await writeTextFile(path, content);
        } catch (error) {
            console.error('Notes text export failed', error);
        }
    }

    /** Export the raw Markdown. A `.ki` body IS Markdown, so this is a copy —
     *  no conversion, nothing lost. The frontmatter is deliberately dropped:
     *  the export is for reading elsewhere, and `title:`/`pinned:`/`tags:`
     *  are KeepItLocal bookkeeping, not document content. */
    async function exportMarkdown() {
        const note = get(activeNote);
        if (!note) return;
        const path = await save({
            title: 'Export note as Markdown',
            defaultPath: `${note.meta.title || 'note'}.md`,
            filters: [{ name: 'Markdown', extensions: ['md'] }],
        });
        if (!path) return;
        try {
            const title = (note.meta.title || '').trim();
            await invoke<{ outputPath: string }>('notes_export_markdown', {
                options: {
                    markdown: note.body || '',
                    outputPath: path,
                    title: title || null,
                },
            });
            toast('Note exported as Markdown', 'success');
        } catch (error) {
            console.error('Notes markdown export failed', error);
            toast(`Markdown export failed: ${String(error)}`, 'error');
        }
    }

    /** Export a self-contained HTML file — styled, with images inlined as
     *  base64 so it renders anywhere with no sidecar folder. Done in Rust
     *  (notes_export_html) rather than with `marked` here, because inlining
     *  attachments means reading files off disk; that stays behind the
     *  notes-folder chokepoint instead of widening the fs capability. */
    let htmlExporting = $state(false);
    async function exportHtml() {
        const note = get(activeNote);
        if (!note || htmlExporting) return;
        const path = await save({
            title: 'Export note as HTML',
            defaultPath: `${note.meta.title || 'note'}.html`,
            filters: [{ name: 'HTML', extensions: ['html'] }],
        });
        if (!path) return;
        htmlExporting = true;
        try {
            const baseDir = await invoke<string>('get_notes_dir').catch(() => '');
            await invoke<{ outputPath: string }>('notes_export_html', {
                options: {
                    markdown: note.body || '',
                    outputPath: path,
                    title: (note.meta.title || '').trim() || 'Untitled note',
                    baseDir,
                },
            });
            toast('Note exported as HTML', 'success');
        } catch (error) {
            console.error('Notes HTML export failed', error);
            toast(`HTML export failed: ${String(error)}`, 'error');
        } finally {
            htmlExporting = false;
        }
    }

    // ── Export as PDF ───────────────────────────────────────────────────────
    /** Export the active note to a styled PDF via Wave 3.1's pipeline:
     *  Markdown → DOCX (docx-rs walks the pulldown-cmark event stream,
     *  building heading/body/list/code/blockquote paragraphs with real
     *  inline bold + italic runs) → PDF (docxide-pdf renders the DOCX
     *  with Word-grade typography). The DOCX intermediate is what
     *  lets us match Word's kerning + line-breaking + list-marker
     *  alignment that direct printpdf rendering couldn't match. */
    let pdfExporting = $state(false);
    async function exportPdf() {
        const note = get(activeNote);
        if (!note || pdfExporting) return;
        const path = await save({
            title: 'Export note as PDF',
            defaultPath: `${note.meta.title || 'note'}.pdf`,
            filters: [{ name: 'PDF', extensions: ['pdf'] }],
        });
        if (!path) return;
        pdfExporting = true;
        try {
            const title = (note.meta.title || '').trim() || 'Untitled note';
            // Notes folder so the backend can resolve relative image srcs
            // (`attachments/x.png`) and embed them in the export.
            const baseDir = await invoke<string>('get_notes_dir').catch(() => '');
            const result = await invoke<{ outputPath: string }>(
                'notes_export_styled_pdf',
                {
                    options: {
                        markdown: note.body || '',
                        outputPath: path,
                        title,
                        baseDir,
                    },
                },
            );
            toast(`Exported "${title}" to PDF`, 'success');
            // Reserved for future polish: an "Open" action toast button
            // that calls revealItemInDir(result.outputPath).
            void result;
        } catch (error) {
            console.error('Notes PDF export failed', error);
            toast(`PDF export failed: ${String(error)}`, 'error');
        } finally {
            pdfExporting = false;
        }
    }

    /* ── Body search ─────────────────────────────────────────────────────
       `preview` is only the first 160 chars of a note (PREVIEW_LEN in
       notes.rs), so matching on it alone made anything past roughly the
       first paragraph unfindable — a note with "invoice #4471" in its
       fourth paragraph simply didn't come up. Title/preview/tag matching
       stays synchronous and instant below; this adds the one signal that
       needs to touch disk, debounced and merged in as it arrives. */
    const BODY_SEARCH_DEBOUNCE_MS = 200;
    type BodySearchHit = { path: string; snippet: string };
    /** Body matches, keyed by path so a content-only result can explain itself. */
    let bodyMatchSnippets = $state<Map<string, string>>(new Map());
    /** The result reached with the search input's arrow keys, if any. */
    let searchSelectedPath = $state<string | null>(null);
    let bodySearchTimer: ReturnType<typeof setTimeout> | null = null;
    /** Guards against an older, slower scan landing after a newer one and
     *  repainting the list with stale matches. */
    let bodySearchSeq = 0;

    $effect(() => {
        const q = query.trim();
        if (bodySearchTimer) clearTimeout(bodySearchTimer);

        // Clear synchronously: a stale match set outliving its query would
        // show notes that don't match what's in the box.
        if (q.length < 2) {
            bodySearchSeq++;
            if (bodyMatchSnippets.size) bodyMatchSnippets = new Map();
            return;
        }

        const seq = ++bodySearchSeq;
        bodySearchTimer = setTimeout(() => {
            bodySearchTimer = null;
            void invoke<BodySearchHit[]>('search_note_bodies', { query: q })
                .then((hits) => {
                    if (seq !== bodySearchSeq) return; // superseded
                    bodyMatchSnippets = new Map(hits.map((hit) => [hit.path, hit.snippet]));
                })
                // Search must degrade to title/preview/tags, never break.
                .catch(() => {
                    if (seq === bodySearchSeq) bodyMatchSnippets = new Map();
                });
        }, BODY_SEARCH_DEBOUNCE_MS);

        return () => {
            if (bodySearchTimer) clearTimeout(bodySearchTimer);
        };
    });

    /* ── Wikilinks ───────────────────────────────────────────────────────
       Both callbacks resolve against the live notes list, so a [[link]]
       starts working the moment its target exists — no reindex, no reload.
       Title matching is case-insensitive and trimmed because that's how
       people actually type a link vs how they typed the title. */
    const normalizeNoteTitle = (title: string) => title.trim().toLocaleLowerCase();

    let notesByTitle = $derived.by(() => {
        const grouped = new Map<string, NoteSummary[]>();
        for (const note of $notes) {
            const key = normalizeNoteTitle(note.title);
            if (!key) continue;
            const matches = grouped.get(key);
            if (matches) matches.push(note);
            else grouped.set(key, [note]);
        }
        return grouped;
    });
    let wikiLinkTitles = $derived($notes.map((note) => note.title));
    const getWikiLinkTitles = () => wikiLinkTitles;

    function findNoteByTitle(title: string) {
        return notesByTitle.get(normalizeNoteTitle(title))?.[0];
    }
    function resolveNoteTitle(title: string): boolean {
        return (notesByTitle.get(normalizeNoteTitle(title))?.length ?? 0) > 0;
    }
    function navigateToNote(title: string): void {
        const hit = findNoteByTitle(title);
        // Unresolved chips remain inert here; the Context drawer offers
        // missing-link creation without making an editor click destructive.
        if (hit) void openNote(hit.path);
    }

    type LinkSource = { path: string; body: string };
    type ParsedLinkSource = { path: string; links: WikiLinkReference[] };

    let contextOpen = $state(false);
    let contextView = $state<'context' | 'graph'>('context');
    let contextLoading = $state(false);
    let contextError = $state('');
    let contextForPath = $state<string | null>(null);
    let contextSources = $state<ParsedLinkSource[]>([]);
    let contextLoadSequence = 0;
    let headingTarget = $state<{ index: number; request: number } | null>(null);
    let headingRequest = 0;
    let headingTargetPath = $state<string | null>(null);

    $effect(() => {
        const path = $activeNote?.path ?? null;
        if (path !== headingTargetPath) {
            headingTargetPath = path;
            headingTarget = null;
        }
    });

    $effect(() => {
        const path = $activeNote?.path;
        if (!contextOpen || !path || contextForPath === path) return;
        void refreshContextSources(path);
    });

    async function refreshContextSources(path = $activeNote?.path): Promise<void> {
        if (!path) return;
        const sequence = ++contextLoadSequence;
        contextLoading = true;
        contextError = '';
        try {
            const sources = await invoke<LinkSource[]>('list_note_link_sources');
            if (sequence !== contextLoadSequence || !contextOpen || path !== $activeNote?.path) return;
            contextSources = sources
                .filter((source) => source.path !== path)
                .map((source) => ({ path: source.path, links: extractWikiLinks(source.body) }))
                .filter((source) => source.links.length > 0);
            contextForPath = path;
        } catch {
            if (sequence !== contextLoadSequence || !contextOpen || path !== $activeNote?.path) return;
            contextSources = [];
            contextForPath = path;
            contextError = 'Could not load linked notes.';
        } finally {
            if (sequence === contextLoadSequence) contextLoading = false;
        }
    }

    function toggleContext(view: 'context' | 'graph' = 'context'): void {
        if (contextOpen && contextView === view) {
            contextOpen = false;
        } else {
            contextOpen = true;
            contextView = view;
        }
        if (contextOpen) return;
        contextLoadSequence++;
        contextSources = [];
        contextForPath = null;
        contextError = '';
    }

    let activeLinkContext = $derived.by(() => {
        if (!contextOpen) return [];
        const seen = new Set<string>();
        return extractWikiLinks($activeNote?.body ?? '').flatMap((link) => {
            const key = normalizeNoteTitle(link.title);
            if (!key || seen.has(key)) return [];
            seen.add(key);
            return [{ ...link, matches: notesByTitle.get(key) ?? [] }];
        });
    });

    let noteOutline = $derived.by(() => {
        if (!contextOpen || !$activeNote) return [];
        let index = 0;
        return marked.lexer($activeNote.body).flatMap((token) => {
            if (token.type !== 'heading') return [];
            return [{ index: index++, depth: token.depth, text: token.text }];
        });
    });

    let backlinks = $derived.by(() => {
        const active = $activeNote;
        if (!contextOpen || !active || contextForPath !== active.path) return [];
        const target = normalizeNoteTitle(active.meta.title);
        if (!target) return [];
        return contextSources
            .flatMap((source) => {
                const count = source.links.filter((link) => normalizeNoteTitle(link.title) === target).length;
                if (!count) return [];
                const note = $notes.find((candidate) => candidate.path === source.path);
                return [
                    {
                        path: source.path,
                        title: note?.title || 'Untitled note',
                        preview: note?.preview || '',
                        count,
                    },
                ];
            })
            .sort((a, b) => a.title.localeCompare(b.title));
    });

    type GraphNode = {
        path: string;
        title: string;
        direction: 'incoming' | 'outgoing' | 'both';
        x: number;
        y: number;
    };
    let graphNodes = $derived.by<GraphNode[]>(() => {
        if (!contextOpen || !$activeNote) return [];
        const linked = new Map<string, Omit<GraphNode, 'x' | 'y'>>();
        for (const backlink of backlinks) {
            linked.set(backlink.path, {
                path: backlink.path,
                title: backlink.title,
                direction: 'incoming',
            });
        }
        for (const link of activeLinkContext) {
            if (link.matches.length !== 1) continue;
            const match = link.matches[0];
            const existing = linked.get(match.path);
            linked.set(match.path, {
                path: match.path,
                title: match.title || link.title,
                direction: existing ? 'both' : 'outgoing',
            });
        }
        const nodes = [...linked.values()].slice(0, 10);
        return nodes.map((node, index) => {
            const angle = -Math.PI / 2 + (index / Math.max(nodes.length, 1)) * Math.PI * 2;
            return { ...node, x: 50 + Math.cos(angle) * 36, y: 50 + Math.sin(angle) * 36 };
        });
    });

    function revealHeading(index: number): void {
        headingTarget = { index, request: ++headingRequest };
    }

    function createLinkedNote(title: string): void {
        const existing = findNoteByTitle(title);
        if (existing) {
            void openNote(existing.path);
            return;
        }
        void newNote(title);
    }

    const RECENT_NOTE_WINDOW_MS = 7 * 24 * 60 * 60 * 1000;
    // ponytail: the cutoff refreshes with note-list changes, add a daily timer only if an
    // always-open window needs exact rollover at the seven-day boundary.
    let recentlyUpdatedNotes = $derived.by(() => {
        const cutoff = Date.now() - RECENT_NOTE_WINDOW_MS;
        return $notes
            .filter((note) => note.modifiedMs >= cutoff)
            .sort((a, b) => b.modifiedMs - a.modifiedMs);
    });
    let inboxNotes = $derived.by(() => $notes.filter((note) => isInboxFolder(note.folder)));

    let filtered = $derived.by(() => {
        const q = query.trim().toLowerCase();
        const f = $currentFolder;
        let list = $notes;
        // Folder filter: null = all, '' = root only, 'Work' = that subfolder.
        if (libraryView === 'folder' && f !== null) {
            list = list.filter((n) => (n.folder ?? '') === f);
        } else if (libraryView === 'inbox') {
            list = inboxNotes;
        } else if (libraryView === 'pinned') {
            list = list.filter((n) => n.pinned);
        } else if (libraryView === 'recent') {
            list = recentlyUpdatedNotes;
        }
        if (tagFilter) {
            const tag = tagFilter.toLocaleLowerCase();
            list = list.filter((n) => n.tags.some((candidate) => candidate.toLocaleLowerCase() === tag));
        }
        if (!q) return list;
        const bodies = bodyMatchSnippets;
        return list.filter(
            (n) =>
                n.title.toLowerCase().includes(q) ||
                n.preview.toLowerCase().includes(q) ||
                n.tags.some((t) => t.toLowerCase().includes(q)) ||
                bodies.has(n.path),
        );
    });

    let selectedSearchResult = $derived.by(() => {
        if (!query.trim() || !searchSelectedPath) return null;
        return filtered.find((note) => note.path === searchSelectedPath) ?? null;
    });

    function moveSearchSelection(direction: -1 | 1): void {
        if (!query.trim() || !filtered.length) return;
        const currentIndex = searchSelectedPath
            ? filtered.findIndex((note) => note.path === searchSelectedPath)
            : -1;
        const nextIndex =
            currentIndex < 0
                ? direction > 0
                    ? 0
                    : filtered.length - 1
                : (currentIndex + direction + filtered.length) % filtered.length;
        const path = filtered[nextIndex].path;
        searchSelectedPath = path;
        requestAnimationFrame(() => {
            Array.from(document.querySelectorAll<HTMLElement>('[data-note-search-result]'))
                .find((element) => element.dataset.noteSearchResult === path)
                ?.scrollIntoView({ block: 'nearest' });
        });
    }

    function handleSearchKeydown(event: KeyboardEvent): void {
        if (event.key === 'ArrowDown') {
            event.preventDefault();
            moveSearchSelection(1);
        } else if (event.key === 'ArrowUp') {
            event.preventDefault();
            moveSearchSelection(-1);
        } else if (event.key === 'Enter' && selectedSearchResult) {
            event.preventDefault();
            void openNote(selectedSearchResult.path);
        }
    }

    let libraryHeading = $derived.by(() => {
        if (tagFilter) return `#${tagFilter}`;
        if (libraryView === 'inbox') return 'Inbox';
        if (libraryView === 'pinned') return 'Pinned notes';
        if (libraryView === 'recent') return 'Recently updated';
        if (libraryView === 'folder') return $currentFolder || 'Unfiled notes';
        return 'All notes';
    });

    function selectLibraryView(view: 'all' | 'recent' | 'pinned'): void {
        libraryView = view;
        tagFilter = null;
        currentFolder.set(null);
    }

    function selectInbox(): void {
        libraryView = 'inbox';
        tagFilter = null;
        currentFolder.set(INBOX_FOLDER);
    }

    function selectFolder(folder: string): void {
        libraryView = 'folder';
        tagFilter = null;
        currentFolder.set(folder);
    }

    function selectTag(tag: string): void {
        tagFilter = tagFilter === tag ? null : tag;
        libraryView = 'all';
        currentFolder.set(null);
    }

    function prepareNoteCreation(): void {
        templatesOpen = false;
        // New notes naturally appear in a real folder and in the recent view.
        // A fresh note is neither pinned nor tagged, and Trash is not a creation
        // destination, so return to the visible all-notes collection first.
        if (libraryView !== 'folder' && libraryView !== 'recent' && libraryView !== 'inbox') {
            selectLibraryView('all');
        }
    }

    async function createNote(): Promise<void> {
        prepareNoteCreation();
        await newNote();
    }

    async function createNoteFromTemplate(template: string): Promise<void> {
        prepareNoteCreation();
        await newNoteFromTemplate(template);
    }

    async function createNotesExamples(): Promise<void> {
        templatesOpen = false;
        if (!(await flushSave())) return;
        const existingTitles = new Set(
            $notes.filter((note) => note.folder === NOTES_EXAMPLES_FOLDER).map((note) => note.title),
        );
        const missingExamples = NOTES_EXAMPLES.filter((example) => !existingTitles.has(example.title));
        try {
            await createNoteFolder(NOTES_EXAMPLES_FOLDER);
            selectFolder(NOTES_EXAMPLES_FOLDER);
            for (const example of missingExamples) await newNote(example.title, example.body);
            const firstExample = $notes.find(
                (note) => note.folder === NOTES_EXAMPLES_FOLDER && note.title === NOTES_EXAMPLES[0].title,
            );
            if (!firstExample) {
                toast('Could not create Notes examples.', 'error');
                return;
            }
            await openNote(firstExample.path);
            toast(missingExamples.length ? 'Created Notes examples.' : 'Notes examples are ready.', 'success');
        } catch (error) {
            toast(`Could not create Notes examples: ${String(error)}`, 'error');
        }
    }

    async function openToday(): Promise<void> {
        const today = new Date();
        const dailyTemplate = $noteTemplates.find((template) => template.toLocaleLowerCase() === 'daily note');
        const title = new Intl.DateTimeFormat(undefined, {
            weekday: 'long',
            month: 'long',
            day: 'numeric',
            year: 'numeric',
        }).format(today);
        const summary = await openTodayNote(localDateKey(today), title, dailyTemplate);
        if (!summary) {
            toast('Could not open today\'s note.', 'error');
            return;
        }
        selectFolder(DAILY_FOLDER);
    }

    let captureDialog = $state<HTMLDialogElement | null>(null);
    let captureTitle = $state('');
    let captureSource = $state('');
    let captureBody = $state('');
    let captureSaving = $state(false);

    type NoteRevision = { id: string; createdMs: number; bytes: number };

    function diffLineKind(line: string): 'add' | 'remove' | 'hunk' | 'file' | null {
        if (line.startsWith('+++') || line.startsWith('---')) return 'file';
        if (line.startsWith('@@')) return 'hunk';
        if (line.startsWith('+')) return 'add';
        if (line.startsWith('-')) return 'remove';
        return null;
    }

    let revisionDialog = $state<HTMLDialogElement | null>(null);
    let historyPath = $state('');
    let revisions = $state<NoteRevision[]>([]);
    let selectedRevision = $state<NoteRevision | null>(null);
    let revisionDiff = $state('');
    let revisionsLoading = $state(false);
    let revisionLoading = $state(false);
    let revisionRestoring = $state(false);
    let revisionError = $state('');

    async function openRevisionHistory(): Promise<void> {
        const note = get(activeNote);
        if (!note || !(await flushSave())) {
            if (note) toast('Could not save this note before opening history.', 'error');
            return;
        }
        // Saving can yield to the UI; never attach history to a note selected
        // while that save was in flight.
        if (get(activeNote)?.path !== note.path) return;

        historyPath = note.path;
        revisions = [];
        selectedRevision = null;
        revisionDiff = '';
        revisionError = '';
        revisionsLoading = true;
        revisionDialog?.showModal();

        try {
            const entries = await invoke<NoteRevision[]>('list_note_revisions', { path: note.path });
            if (historyPath !== note.path) return;
            revisions = entries;
            if (entries[0]) await selectRevision(entries[0]);
        } catch (error) {
            if (historyPath === note.path) revisionError = String(error);
        } finally {
            if (historyPath === note.path) revisionsLoading = false;
        }
    }

    async function selectRevision(revision: NoteRevision): Promise<void> {
        const path = historyPath;
        if (!path) return;
        selectedRevision = revision;
        revisionDiff = '';
        revisionError = '';
        revisionLoading = true;
        try {
            const diff = await invoke<string>('diff_note_revision', { path, id: revision.id });
            if (historyPath === path && selectedRevision?.id === revision.id) revisionDiff = diff;
        } catch (error) {
            if (historyPath === path && selectedRevision?.id === revision.id) revisionError = String(error);
        } finally {
            if (historyPath === path && selectedRevision?.id === revision.id) revisionLoading = false;
        }
    }

    async function restoreSelectedRevision(): Promise<void> {
        const revision = selectedRevision;
        const note = get(activeNote);
        if (!revision || !note || note.path !== historyPath || revisionRestoring) return;

        // A native dialog sits in the browser top layer, above our app-level
        // confirmation surface. Close it first so the confirmation is usable.
        revisionDialog?.close();
        const ok = await confirm(
            `Restore the version from ${relTime(revision.createdMs)}? Your current saved version will remain available in history.`,
            { title: 'Restore this version?', confirmLabel: 'Restore version', kind: 'warning', danger: true },
        );
        if (!ok) {
            revisionDialog?.showModal();
            return;
        }

        revisionRestoring = true;
        try {
            if (!(await restoreActiveNoteRevision(revision.id))) {
                toast('Could not restore this version. Your current note is unchanged.', 'error');
                return;
            }
            toast('Version restored. The version you replaced is still in history.', 'success');
        } finally {
            revisionRestoring = false;
        }
    }

    function openCapture(): void {
        captureDialog?.showModal();
    }

    async function pasteCapture(): Promise<void> {
        try {
            captureBody = await navigator.clipboard.readText();
        } catch {
            toast('Could not read the clipboard.', 'error');
        }
    }

    async function importCapture(): Promise<void> {
        const path = await openFileDialog({
            title: 'Import Markdown or text',
            multiple: false,
            filters: [{ name: 'Markdown or text', extensions: ['md', 'markdown', 'mdown', 'txt'] }],
        });
        if (typeof path !== 'string') return;
        try {
            captureBody = await readTextFile(path);
            captureTitle ||= (path.split(/[\\/]/).pop() ?? '').replace(/\.[^.]+$/, '');
        } catch (error) {
            toast(`Could not import file: ${String(error)}`, 'error');
        }
    }

    function captureSourceLine(value: string): string {
        const source = value.trim();
        if (!source) return '';
        try {
            const url = new URL(source);
            if (url.protocol === 'https:' || url.protocol === 'http:') return `[Source](${url.href})`;
        } catch {
            // Plain source text is still useful when it is not a web URL.
        }
        return `Source: ${source}`;
    }

    async function saveCapture(): Promise<void> {
        const body = sanitizeImportedMarkdown(stripOuterMarkdownFence(captureBody)).trim();
        if (!body) {
            toast('Add or import something to capture.', 'error');
            return;
        }
        if (body.length > 1_000_000) {
            toast('Capture is too large. Import a smaller text file.', 'error');
            return;
        }
        captureSaving = true;
        try {
            const source = captureSourceLine(captureSource);
            const summary = await captureNote(source ? `${body}\n\n${source}` : body, captureTitle);
            if (!summary) {
                toast('Could not save capture.', 'error');
                return;
            }
            captureDialog?.close();
            captureTitle = '';
            captureSource = '';
            captureBody = '';
            toast(`Saved "${summary.title}" to Inbox`, 'success');
        } finally {
            captureSaving = false;
        }
    }

    function folderCount(folder: string): number {
        return $notes.filter((note) => (note.folder ?? '') === folder).length;
    }

    function tagCount(tag: string): number {
        const normalized = tag.toLocaleLowerCase();
        return $notes.filter((note) =>
            note.tags.some((candidate) => candidate.toLocaleLowerCase() === normalized),
        ).length;
    }

    function searchMatchReason(note: (typeof $notes)[number]): 'Title' | 'Preview' | 'Tag' | 'Content' | null {
        const q = query.trim().toLocaleLowerCase();
        if (!q) return null;
        if (note.title.toLocaleLowerCase().includes(q)) return 'Title';
        if (note.tags.some((tag) => tag.toLocaleLowerCase().includes(q))) return 'Tag';
        if (note.preview.toLocaleLowerCase().includes(q)) return 'Preview';
        return bodyMatchSnippets.has(note.path) ? 'Content' : null;
    }

    // ── Folders ──────────────────────────────────────────────────────────
    let addingFolder = $state(false);
    let newFolderName = $state('');

    // The active note's current folder — NoteDoc has no `folder`, so look it up
    // from the list summaries by path.
    let activeFolder = $derived.by(() => {
        const p = $activeNote?.path;
        if (!p) return '';
        return $notes.find((n) => n.path === p)?.folder ?? '';
    });

    async function submitNewFolder() {
        const name = newFolderName.trim();
        newFolderName = '';
        addingFolder = false;
        if (!name) return;
        try {
            await createNoteFolder(name);
            selectFolder(name);
            toast(`Created folder "${name}"`, 'success');
        } catch (error) {
            toast(`Could not create folder: ${String(error)}`, 'error');
        }
    }

    /* ── Trash ───────────────────────────────────────────────────────────
       `.trash` was write-only: delete_note moved notes in and nothing could
       list, restore, or empty it — recovery meant opening Explorer. */
    let trashItems = $state<TrashedNote[]>([]);
    let trashLoading = $state(false);

    async function openTrash() {
        libraryView = 'trash';
        tagFilter = null;
        currentFolder.set(null);
        trashLoading = true;
        try {
            trashItems = await listTrashedNotes();
        } finally {
            trashLoading = false;
        }
    }

    async function restoreFromTrash(item: TrashedNote) {
        try {
            await restoreTrashedNote(item.path);
            trashItems = trashItems.filter((t) => t.path !== item.path);
            toast(`Restored ${item.kind === 'folder' ? 'folder' : 'note'} "${item.title}"`, 'success');
        } catch (error) {
            toast(`Could not restore: ${String(error)}`, 'error');
        }
    }

    async function purgeFromTrash(item: TrashedNote) {
        const noun = item.kind === 'folder' ? 'folder' : 'note';
        const ok = await confirm(
            `This ${noun}, "${item.title}", will be gone for good. This cannot be undone.`,
            {
                title: 'Delete permanently?',
                kind: 'warning',
                confirmLabel: 'Delete forever',
                danger: true,
            },
        );
        if (!ok) return;
        try {
            await deleteTrashedNote(item.path);
            trashItems = trashItems.filter((t) => t.path !== item.path);
        } catch (error) {
            toast(`Could not delete: ${String(error)}`, 'error');
        }
    }

    async function emptyTrash() {
        const ok = await confirm(
            `${trashItems.length} item${trashItems.length === 1 ? '' : 's'} will be deleted for good. This cannot be undone.`,
            {
                title: 'Empty the trash?',
                kind: 'warning',
                confirmLabel: 'Empty trash',
                danger: true,
            },
        );
        if (!ok) return;
        try {
            const n = await emptyNoteTrash();
            trashItems = [];
            toast(`Deleted ${n} item${n === 1 ? '' : 's'}`, 'success');
        } catch (error) {
            toast(`Could not empty trash: ${String(error)}`, 'error');
        }
    }

    let renamingFolder = $state<string | null>(null);
    let renameFolderName = $state('');
    let renameFolderInput = $state<HTMLInputElement | null>(null);

    function isInFolder(folder: string, parent: string): boolean {
        return folder === parent || folder.startsWith(`${parent}/`);
    }

    async function startRenamingFolder(folder: string): Promise<void> {
        renameFolderName = folder;
        renamingFolder = folder;
        await tick();
        renameFolderInput?.focus();
        renameFolderInput?.select();
    }

    async function submitRenameFolder() {
        const from = renamingFolder;
        const to = renameFolderName.trim();
        renameFolderName = '';
        renamingFolder = null;
        if (!from || !to || from === to) return;
        try {
            const activeIsInRenamedFolder = isInFolder(activeFolder, from);
            if (activeIsInRenamedFolder && !(await flushSave())) {
                toast('Could not save the open note before renaming this folder.', 'error');
                return;
            }
            await renameNoteFolder(from, to);
            if (activeIsInRenamedFolder) {
                activeNote.set(null);
                selectFolder(to);
                toast(`Renamed folder to "${to}". Reopen the moved note to continue.`, 'success');
            } else {
                toast(`Renamed folder to "${to}"`, 'success');
            }
        } catch (error) {
            toast(`Could not rename folder: ${String(error)}`, 'error');
        }
    }

    async function delFolder(folder: string) {
        const count = $notes.filter((n) => isInFolder(n.folder ?? '', folder)).length;
        const ok = await confirm(
            count > 0
                ? `Move folder "${folder}" and its ${count} note${count === 1 ? '' : 's'} to the trash?`
                : `Delete the empty folder "${folder}"?`,
            { title: 'Delete folder', confirmLabel: 'Move to trash', danger: true },
        );
        if (!ok) return;
        try {
            const activeIsInDeletedFolder = isInFolder(activeFolder, folder);
            if (activeIsInDeletedFolder && !(await flushSave())) {
                toast('Could not save the open note before deleting this folder.', 'error');
                return;
            }
            await deleteNoteFolder(folder);
            if (activeIsInDeletedFolder) {
                activeNote.set(null);
                if ($notes.length) await openNote($notes[0].path);
            }
            if (libraryView === 'folder' && $currentFolder === null) libraryView = 'all';
            toast(`Folder "${folder}" moved to trash`, 'success');
        } catch (error) {
            toast(`Could not delete folder: ${String(error)}`, 'error');
        }
    }

    async function moveActiveNote(target: string) {
        const p = $activeNote?.path;
        if (!p || target === activeFolder) return;
        try {
            if (!(await flushSave())) {
                toast('Could not save this note before moving it. Please try again.', 'error');
                return;
            }
            noteMenuOpen = false;
            const newPath = await moveNote(p, target);
            await openNote(newPath);
            toast(target ? `Moved to "${target}"` : 'Moved to root', 'success');
        } catch (error) {
            toast(`Could not move note: ${String(error)}`, 'error');
        }
    }

    async function resolveExternalConflict(action: 'reload' | 'keep'): Promise<void> {
        const resolved = action === 'reload' ? await reloadActiveNote() : await keepActiveNoteChanges();
        if (!resolved) {
            toast(
                action === 'reload'
                    ? 'Could not reload the external note. Your local draft is still open.'
                    : 'Could not save your local changes. Please try again.',
                'error',
            );
        }
    }

    onMount(() => {
        void (async () => {
            await initNotesStore();
            // Fire-and-forget: templates are a side affordance and must never
            // hold up the note list.
            void refreshNoteTemplates();
            if (!get(activeNote) && get(notes).length) {
                await openNote(get(notes)[0].path);
            }
        })();

        // Don't lose the last edit when the user leaves: flush on window blur
        // and on tab/window hide (both fire while the app is alive, so the async
        // write completes), plus a best-effort beforeunload for a hard close.
        // flushSave is a no-op unless there are unsaved changes (dirty guard).
        const flush = () => void flushSave();
        const onVisibility = () => {
            if (document.hidden) flush();
        };
        window.addEventListener('blur', flush);
        window.addEventListener('beforeunload', flush);
        document.addEventListener('visibilitychange', onVisibility);

        return () => {
            window.removeEventListener('blur', flush);
            window.removeEventListener('beforeunload', flush);
            document.removeEventListener('visibilitychange', onVisibility);
            flush();
        };
    });

    function relTime(ms: number): string {
        const diff = Date.now() - ms;
        const m = Math.floor(diff / 60000);
        if (m < 1) return 'just now';
        if (m < 60) return `${m}m ago`;
        const h = Math.floor(m / 60);
        if (h < 24) return `${h}h ago`;
        const d = Math.floor(h / 24);
        if (d < 7) return `${d}d ago`;
        return new Date(ms).toLocaleDateString();
    }
</script>

<!-- Template menu dismissal. Both guards are cheap no-ops while it's closed.
     pointerdown (not click) so the menu is gone before the click lands on
     whatever is underneath — otherwise dismissing it also activates that. -->
<svelte:window
    onkeydown={(e) => {
        if (e.key !== 'Escape') return;
        if (templatesOpen) templatesOpen = false;
        if (noteMenuOpen) noteMenuOpen = false;
    }}
    onpointerdown={(e) => {
        const target = e.target as Node;
        if (templatesOpen && tplEl && !tplEl.contains(target)) templatesOpen = false;
        if (noteMenuOpen && noteMenuEl && !noteMenuEl.contains(target)) noteMenuOpen = false;
    }}
/>

<div class="notes" class:is-focus={focusMode}>
    <!-- ─── Left: note list ───────────────────────────────────────── -->
    <aside class="notes-list">
        <header class="notes-list-head">
            <div class="notes-brand">
                <NotebookPen class="notes-brand-ico" />
                <span>Notes</span>
            </div>
            <div class="notes-head-actions">
                <button
                    type="button"
                    class="notes-icon-btn"
                    title="Open notes folder"
                    aria-label="Open notes folder"
                    onclick={() => void openNotesFolder()}
                >
                    <FolderOpen class="notes-icon-btn-ico" />
                </button>
                <button
                    type="button"
                    class="notes-icon-btn"
                    title="Capture to Inbox"
                    aria-label="Capture to Inbox"
                    onclick={openCapture}
                >
                    <ClipboardPlus class="notes-icon-btn-ico" />
                </button>
                <div class="notes-new-group" bind:this={tplEl}>
                    <button
                        type="button"
                        class="notes-new"
                        title="New note (creates a .ki file)"
                        onclick={() => void createNote()}
                    >
                        <Plus class="notes-new-ico" />
                        New
                    </button>
                    <div class="notes-tpl">
                        <button
                            type="button"
                            class="notes-tpl-btn"
                            title="New note from template"
                            aria-label="New note from template"
                            aria-expanded={templatesOpen}
                            onclick={() => (templatesOpen = !templatesOpen)}
                        >
                            <ChevronDown class="notes-tpl-ico" />
                        </button>
                        {#if templatesOpen}
                            <div class="notes-tpl-menu" role="menu" tabindex="-1">
                                {#each $noteTemplates as tpl (tpl)}
                                    <button
                                        type="button"
                                        class="notes-tpl-opt"
                                        role="menuitem"
                                        onclick={() => {
                                            templatesOpen = false;
                                            void createNoteFromTemplate(tpl);
                                        }}
                                    >
                                        {tpl}
                                    </button>
                                {/each}
                                {#if !$noteTemplates.length}
                                    <p class="notes-tpl-hint">
                                        Drop a <code>.ki</code> note into the templates folder to start
                                        a new note from it.
                                    </p>
                                {/if}
                                <button
                                    type="button"
                                    class="notes-tpl-opt"
                                    role="menuitem"
                                    onclick={() => void createNotesExamples()}
                                >
                                    Create Notes examples
                                </button>
                                <button
                                    type="button"
                                    class="notes-tpl-opt notes-tpl-manage"
                                    role="menuitem"
                                    onclick={() => {
                                        templatesOpen = false;
                                        void openNoteTemplatesFolder();
                                    }}
                                >
                                    {$noteTemplates.length ? 'Manage templates…' : 'Create templates folder…'}
                                </button>
                            </div>
                        {/if}
                    </div>
                </div>
            </div>
        </header>

        <div class="notes-search">
            <Search class="notes-search-ico" />
            <input
                class="notes-search-input"
                placeholder="Search notes…"
                value={query}
                oninput={(event) => {
                    query = event.currentTarget.value;
                    searchSelectedPath = null;
                }}
                onkeydown={handleSearchKeydown}
                use:escToClear={() => (query = '')}
                spellcheck="false"
                autocomplete="off"
            />
        </div>

        <details class="notes-library">
            <summary class="notes-library-toggle">
                <span class="notes-library-toggle-title">
                    <span>Library</span>
                </span>
                <ChevronDown class="notes-library-toggle-chevron" />
            </summary>
            <nav class="notes-library-content" aria-label="Notes library">
            <section class="notes-library-section">
                <button
                    type="button"
                    class="notes-library-item"
                    class:is-active={libraryView === 'all' && !tagFilter}
                    onclick={() => selectLibraryView('all')}
                >
                    <NotebookPen class="notes-library-ico" />
                    <span>All notes</span>
                    <span class="notes-library-count">{$notes.length}</span>
                </button>
                <button
                    type="button"
                    class="notes-library-item"
                    class:is-active={libraryView === 'inbox' && !tagFilter}
                    onclick={selectInbox}
                >
                    <Inbox class="notes-library-ico" />
                    <span>Inbox</span>
                    {#if inboxNotes.length}
                        <span class="notes-library-count">{inboxNotes.length}</span>
                    {/if}
                </button>
                <button
                    type="button"
                    class="notes-library-item notes-library-today"
                    title="Open today's daily note"
                    onclick={() => void openToday()}
                >
                    <CalendarDays class="notes-library-ico" />
                    <span>Today</span>
                </button>
                <button
                    type="button"
                    class="notes-library-item"
                    class:is-active={libraryView === 'recent' && !tagFilter}
                    onclick={() => selectLibraryView('recent')}
                >
                    <Clock3 class="notes-library-ico" />
                    <span>Recently updated</span>
                    <span class="notes-library-count">{recentlyUpdatedNotes.length}</span>
                </button>
                <button
                    type="button"
                    class="notes-library-item"
                    class:is-active={libraryView === 'pinned' && !tagFilter}
                    onclick={() => selectLibraryView('pinned')}
                >
                    <Pin class="notes-library-ico" />
                    <span>Pinned</span>
                    {#if $notes.some((note) => note.pinned)}
                        <span class="notes-library-count"
                            >{$notes.filter((note) => note.pinned).length}</span
                        >
                    {/if}
                </button>
                <button
                type="button"
                class="notes-library-item notes-library-trash"
                class:is-active={libraryView === 'trash'}
                aria-current={libraryView === 'trash' ? 'page' : undefined}
                onclick={() => void openTrash()}
                >
                <Trash2 class="notes-library-ico" />
                <span>Trash</span>
                {#if trashItems.length}
                    <span class="notes-library-count">{trashItems.length}</span>
                {/if}
            </button>
            </section>

            <section class="notes-library-section notes-library-folders">
                <div class="notes-library-section-head">
                    <p class="notes-library-label">Folders</p>
                    <button
                        type="button"
                        class="notes-library-add"
                        title="New folder"
                        aria-label="New folder"
                        onclick={() => (addingFolder = true)}
                    >
                        <FolderPlus class="notes-library-ico" />
                    </button>
                </div>
                {#if addingFolder}
                    <input
                        class="notes-library-input"
                        bind:value={newFolderName}
                        placeholder="Folder name"
                        spellcheck="false"
                        autocomplete="off"
                        onkeydown={(e) => {
                            if (e.key === 'Enter') void submitNewFolder();
                            else if (e.key === 'Escape') {
                                addingFolder = false;
                                newFolderName = '';
                            }
                        }}
                        onblur={() => void submitNewFolder()}
                    />
                {/if}
                {#each $noteFolders.filter((folder) => !isInboxFolder(folder)) as f (f)}
                    <div
                        class="notes-folder-row"
                        class:is-active={libraryView === 'folder' && $currentFolder === f}
                    >
                        {#if renamingFolder === f}
                            <input
                                class="notes-library-input notes-folder-rename-input"
                                bind:this={renameFolderInput}
                                bind:value={renameFolderName}
                                placeholder="Rename folder"
                                spellcheck="false"
                                autocomplete="off"
                                onkeydown={(e) => {
                                    if (e.key === 'Enter') void submitRenameFolder();
                                    else if (e.key === 'Escape') {
                                        renamingFolder = null;
                                        renameFolderName = '';
                                    }
                                }}
                                onblur={() => void submitRenameFolder()}
                            />
                        {:else}
                            <button
                                type="button"
                                class="notes-library-item"
                                class:is-active={libraryView === 'folder' && $currentFolder === f}
                                title={f}
                                onclick={() => selectFolder(f)}
                            >
                                <Folder class="notes-library-ico" />
                                <span class="notes-library-name">{f}</span>
                                <span class="notes-library-count">{folderCount(f)}</span>
                            </button>
                            <div class="notes-folder-hover-actions">
                                <button
                                    type="button"
                                    class="notes-folder-hover-action"
                                    title={`Rename ${f}`}
                                    aria-label={`Rename ${f}`}
                                    onclick={() => void startRenamingFolder(f)}
                                >
                                    <Pencil />
                                </button>
                                <button
                                    type="button"
                                    class="notes-folder-hover-action is-danger"
                                    title={`Delete ${f}`}
                                    aria-label={`Delete ${f}`}
                                    onclick={() => void delFolder(f)}
                                >
                                    <Trash2 />
                                </button>
                            </div>
                        {/if}
                    </div>
                {/each}
                {#if !$noteFolders.some((folder) => !isInboxFolder(folder)) && !addingFolder}
                    <p class="notes-library-hint">Create folders for projects and topics.</p>
                {/if}
            </section>

            {#if $allTags.length}
                <section class="notes-library-section notes-library-tags">
                    <div class="notes-library-section-head">
                        <p class="notes-library-label">Tags</p>
                        <Tags class="notes-library-section-ico" />
                    </div>
                    {#each $allTags as tag (tag)}
                        <button
                            type="button"
                            class="notes-library-item notes-library-tag"
                            class:is-active={tagFilter === tag}
                            onclick={() => selectTag(tag)}
                        >
                            <span class="notes-library-tag-mark">#</span>
                            <span class="notes-library-name">{tag}</span>
                            <span class="notes-library-count">{tagCount(tag)}</span>
                        </button>
                    {/each}
                </section>
            {/if}

            <div class="notes-library-divider"></div>
            </nav>
        </details>

        <div class="notes-scroll">
            {#if libraryView === 'trash'}
                <div class="notes-results-head">
                    <div>
                        <h2>Trash</h2>
                    </div>
                    {#if trashItems.length}
                        <button type="button" class="notes-text-action" onclick={() => void emptyTrash()}>
                            Empty trash
                        </button>
                    {/if}
                </div>
                <div class="notes-trash">
                    {#if trashLoading}
                        <p class="notes-trash-msg">Loading…</p>
                    {:else if !trashItems.length}
                        <p class="notes-trash-msg">
                            Nothing here. Deleted notes and folders can be restored from here.
                        </p>
                    {:else}
                        {#each trashItems as item (item.path)}
                            <div class="notes-trash-row">
                                <span class="notes-trash-name" title={item.title}>
                                    {#if item.kind === 'folder'}
                                        <Folder class="notes-trash-kind" aria-hidden="true" />
                                    {:else}
                                        <FileText class="notes-trash-kind" aria-hidden="true" />
                                    {/if}
                                    {item.title}
                                </span>
                                <span class="notes-trash-when">{relTime(item.deletedMs)}</span>
                                <button
                                    type="button"
                                    class="notes-trash-act"
                                    title={item.kind === 'folder' ? 'Restore folder' : 'Restore note'}
                                    aria-label={`Restore ${item.kind === 'folder' ? 'folder' : 'note'} ${item.title}`}
                                    onclick={() => void restoreFromTrash(item)}
                                >
                                    <RotateCcw class="notes-trash-act-ico" />
                                </button>
                                <button
                                    type="button"
                                    class="notes-trash-act notes-trash-act-danger"
                                    title="Delete permanently"
                                    aria-label={`Permanently delete ${item.title}`}
                                    onclick={() => void purgeFromTrash(item)}
                                >
                                    <Trash2 class="notes-trash-act-ico" />
                                </button>
                            </div>
                        {/each}
                    {/if}
                </div>
            {:else}
                <div class="notes-results-head">
                    <div>
                        {#if query.trim()}
                            <p class="notes-results-kicker">Search results for {query.trim()}</p>
                        {/if}
                        <h2>{libraryHeading}</h2>
                    </div>
                    {#if $notesReady}
                        <div class="notes-results-summary">
                            <span class="notes-results-count"
                                >{filtered.length} {filtered.length === 1 ? 'note' : 'notes'}</span
                            >
                            {#if query.trim() && filtered.length}
                                <span
                                    class="notes-results-keyhint"
                                    aria-label="Use up and down arrows to navigate results, then Enter to open"
                                >
                                    <kbd aria-hidden="true">↑↓</kbd><kbd aria-hidden="true">↵</kbd>
                                </span>
                            {/if}
                        </div>
                    {/if}
                </div>
                {#if !$notesReady}
                    <div class="notes-msg">Loading…</div>
                {:else if filtered.length === 0}
                    <div class="notes-msg">
                        {query
                            ? 'No notes match this search.'
                            : tagFilter
                              ? 'No notes use this tag yet.'
                              : libraryView === 'recent'
                                ? 'No notes have been updated in the last 7 days.'
                              : 'No notes here yet. Create your first one.'}
                    </div>
                {:else}
                    {#each filtered as note (note.path)}
                        {@const matchReason = searchMatchReason(note)}
                        <div
                            class="notes-row-wrap"
                            class:is-active={$activeNote?.path === note.path}
                            class:is-search-selected={query.trim() && searchSelectedPath === note.path}
                        >
                            <button
                                type="button"
                                class="notes-row"
                                class:is-active={$activeNote?.path === note.path}
                                data-note-search-result={query.trim() ? note.path : undefined}
                                onclick={() => {
                                    searchSelectedPath = note.path;
                                    void openNote(note.path);
                                }}
                            >
                                <div class="notes-row-main">
                                    <div class="notes-row-title">
                                        {note.title || 'Untitled'}
                                        {#if note.pinned}<Pin class="notes-row-pin" />{/if}
                                    </div>
                                    <div class="notes-row-preview">
                                        {matchReason === 'Content'
                                            ? bodyMatchSnippets.get(note.path) || note.preview || 'Content match'
                                            : note.preview || 'Empty note'}
                                    </div>
                                    {#if matchReason}
                                        <div class="notes-row-match">{matchReason} match</div>
                                    {/if}
                                    {#if note.tags.length}
                                        <div class="notes-row-tags">
                                            {#each note.tags.slice(0, 3) as tag (tag)}
                                                <span class="notes-row-tag">{tag}</span>
                                            {/each}
                                            {#if note.tags.length > 3}
                                                <span class="notes-row-tag-more"
                                                    >+{note.tags.length - 3}</span
                                                >
                                            {/if}
                                        </div>
                                    {/if}
                                    <div class="notes-row-meta">{relTime(note.modifiedMs)}</div>
                                </div>
                            </button>
                            <button
                                type="button"
                                class="notes-row-act"
                                class:is-on={note.pinned}
                                title={note.pinned ? 'Unpin' : 'Pin'}
                                aria-label={`${note.pinned ? 'Unpin' : 'Pin'} ${note.title || 'Untitled'}`}
                                onclick={() => void togglePin(note.path)}
                            >
                                <Pin class="notes-row-act-ico" />
                            </button>
                        </div>
                    {/each}
                {/if}
            {/if}
        </div>
    </aside>

    <!-- ─── Right: editor ─────────────────────────────────────────── -->
    <section class="notes-editor">
        {#if $activeNote}
            <header class="notes-editor-head">
                <div class="notes-editor-heading">
                    <div class="notes-breadcrumb">
                        <Folder class="notes-breadcrumb-ico" />
                        <span>{activeFolder || 'All notes'}</span>
                    </div>
                    <input
                        class="notes-title"
                        bind:value={titleBuffer}
                        oninput={() => setActiveTitle(titleBuffer)}
                        placeholder="Untitled"
                        spellcheck="false"
                    />
                </div>
                <div class="notes-editor-actions">
                    {#if $saveStatus === 'saving'}
                        <span class="notes-save-status" aria-live="polite">Saving…</span>
                    {:else if $saveStatus === 'saved'}
                        <span class="notes-save-status is-saved" aria-live="polite">
                            <Check class="notes-save-ico" /> Saved
                        </span>
                    {/if}
                    <button
                        type="button"
                        class="notes-icon-btn"
                        class:is-on={focusMode}
                        title={focusMode ? 'Leave focus mode' : 'Focus on this note'}
                        aria-label={focusMode ? 'Leave focus mode' : 'Focus on this note'}
                        aria-pressed={focusMode}
                        onclick={() => (focusMode = !focusMode)}
                    >
                        {#if focusMode}<Minimize2 class="notes-icon-btn-ico" />{:else}<Maximize2 class="notes-icon-btn-ico" />{/if}
                    </button>
                    <button
                        type="button"
                        class="notes-icon-btn"
                        class:is-on={!!splitNote || splitLoading}
                        title={splitNote ? 'Close split preview' : 'Open split preview'}
                        aria-label={splitNote ? 'Close split preview' : 'Open split preview'}
                        aria-pressed={!!splitNote}
                        onclick={toggleSplit}
                    >
                        <Columns2 class="notes-icon-btn-ico" />
                    </button>
                    <button
                        type="button"
                        class="notes-icon-btn"
                        class:is-on={$activeNote.meta.pinned}
                        title={$activeNote.meta.pinned ? 'Unpin' : 'Pin'}
                        aria-label="Pin note"
                        onclick={() => void togglePin($activeNote.path)}
                    >
                        <Pin class="notes-icon-btn-ico" />
                    </button>
                    <button
                        type="button"
                        class="notes-icon-btn"
                        class:is-on={contextOpen && contextView === 'context'}
                        title={contextOpen && contextView === 'context' ? 'Close note context' : 'Open note context'}
                        aria-label={contextOpen && contextView === 'context' ? 'Close note context' : 'Open note context'}
                        aria-expanded={contextOpen && contextView === 'context'}
                        onclick={() => toggleContext('context')}
                    >
                        <PanelRightOpen class="notes-icon-btn-ico" />
                    </button>
                    <button
                        type="button"
                        class="notes-icon-btn"
                        class:is-on={contextOpen && contextView === 'graph'}
                        title="Open local graph"
                        aria-label="Open local graph"
                        aria-pressed={contextOpen && contextView === 'graph'}
                        onclick={() => toggleContext('graph')}
                    >
                        <Network class="notes-icon-btn-ico" />
                    </button>
                    <div class="notes-more" bind:this={noteMenuEl}>
                        <button
                            type="button"
                            class="notes-icon-btn"
                            class:is-on={noteMenuOpen}
                            title="Note actions"
                            aria-label="Note actions"
                            aria-expanded={noteMenuOpen}
                            onclick={() => (noteMenuOpen = !noteMenuOpen)}
                        >
                            <Ellipsis class="notes-icon-btn-ico" />
                        </button>
                        {#if noteMenuOpen}
                            <div class="notes-more-menu" role="menu" tabindex="-1">
                                {#if $noteFolders.length || activeFolder || !isInboxFolder(activeFolder)}
                                    <p class="notes-more-label">Move to</p>
                                    <button
                                        type="button"
                                        class="notes-more-option"
                                        role="menuitem"
                                        disabled={activeFolder === ''}
                                        onclick={() => void moveActiveNote('')}
                                    >
                                        <Folder class="notes-more-ico" /> Root
                                    </button>
                                    <button
                                        type="button"
                                        class="notes-more-option"
                                        role="menuitem"
                                        disabled={isInboxFolder(activeFolder)}
                                        onclick={() => void moveActiveNote(INBOX_FOLDER)}
                                    >
                                        <Inbox class="notes-more-ico" /> Inbox
                                    </button>
                                    {#each $noteFolders.filter((folder) => !isInboxFolder(folder)) as f (f)}
                                        <button
                                            type="button"
                                            class="notes-more-option"
                                            role="menuitem"
                                            disabled={activeFolder === f}
                                            onclick={() => void moveActiveNote(f)}
                                        >
                                            <Folder class="notes-more-ico" /> {f}
                                        </button>
                                    {/each}
                                    <div class="notes-more-divider"></div>
                                {/if}
                                <p class="notes-more-label">Recovery</p>
                                <button
                                    type="button"
                                    class="notes-more-option"
                                    role="menuitem"
                                    onclick={() => {
                                        noteMenuOpen = false;
                                        void openRevisionHistory();
                                    }}
                                >
                                    <History class="notes-more-ico" /> Version history
                                </button>
                                <div class="notes-more-divider"></div>
                                <p class="notes-more-label">Export</p>
                                <button
                                    type="button"
                                    class="notes-more-option"
                                    role="menuitem"
                                    onclick={() => {
                                        noteMenuOpen = false;
                                        void exportMarkdown();
                                    }}
                                >
                                    <Download class="notes-more-ico" />
                                    <span>Markdown</span><span class="notes-export-ext">.md</span>
                                </button>
                                <button
                                    type="button"
                                    class="notes-more-option"
                                    role="menuitem"
                                    disabled={htmlExporting}
                                    onclick={() => {
                                        noteMenuOpen = false;
                                        void exportHtml();
                                    }}
                                >
                                    <Download class="notes-more-ico" />
                                    <span>HTML</span><span class="notes-export-ext">.html</span>
                                </button>
                                <button
                                    type="button"
                                    class="notes-more-option"
                                    role="menuitem"
                                    disabled={pdfExporting}
                                    onclick={() => {
                                        noteMenuOpen = false;
                                        void exportPdf();
                                    }}
                                >
                                    <Download class="notes-more-ico" />
                                    <span>PDF</span><span class="notes-export-ext">.pdf</span>
                                </button>
                                <button
                                    type="button"
                                    class="notes-more-option"
                                    role="menuitem"
                                    onclick={() => {
                                        noteMenuOpen = false;
                                        void exportText();
                                    }}
                                >
                                    <Download class="notes-more-ico" />
                                    <span>Plain text</span><span class="notes-export-ext">.txt</span>
                                </button>
                                <div class="notes-more-divider"></div>
                                <button
                                    type="button"
                                    class="notes-more-option notes-more-delete"
                                    role="menuitem"
                                    onclick={() => {
                                        noteMenuOpen = false;
                                        void removeNote($activeNote.path);
                                    }}
                                >
                                    <Trash2 class="notes-more-ico" /> Delete note
                                </button>
                            </div>
                        {/if}
                    </div>
                </div>
            </header>
            {#if $externalNoteConflict?.path === $activeNote.path}
                <div class="notes-external-change" role="alert">
                    <div class="notes-external-change-copy">
                        <strong>
                            {$externalNoteConflict.kind === 'missing'
                                ? 'This note was removed outside KeepItLocal.'
                                : 'This note changed outside KeepItLocal.'}
                        </strong>
                        <span>
                            {$externalNoteConflict.kind === 'missing'
                                ? 'Your local edits are still here. Keep mine will restore the note.'
                                : 'Your local edits are still here. Choose which version to keep.'}
                        </span>
                    </div>
                    <div class="notes-external-change-actions">
                        {#if $externalNoteConflict.kind !== 'missing'}
                            <button
                                type="button"
                                class="notes-external-change-btn"
                                onclick={() => void resolveExternalConflict('reload')}
                            >
                                Reload
                            </button>
                        {/if}
                        <button
                            type="button"
                            class="notes-external-change-btn is-primary"
                            onclick={() => void resolveExternalConflict('keep')}
                        >
                            Keep mine
                        </button>
                    </div>
                </div>
            {/if}
            <!-- Outside the {#key}: only TipTap needs remounting per note;
                 the tag row just takes new props. -->
            <div class="notes-tags-row">
                <span class="notes-tags-label">Tags</span>
                <NoteTagInput
                    tags={$activeNote.meta.tags}
                    suggestions={$allTags}
                    onChange={setActiveTags}
                />
            </div>
            <div class="notes-workspace" bind:this={workspaceEl}>
                {#key $activeNote.path}
                    <div class="notes-editor-body">
                        <NotesEditor
                            value={$activeNote.body}
                            onChange={setActiveBody}
                            noteTitles={getWikiLinkTitles}
                            resolveNote={resolveNoteTitle}
                            onNavigateNote={navigateToNote}
                            {headingTarget}
                        />
                    </div>
                {/key}
                {#if splitNote || splitLoading}
                    <div
                        class="notes-split-divider"
                        class:is-dragging={splitDragging}
                        role="slider"
                        aria-label="Resize split preview"
                        aria-orientation="vertical"
                        aria-valuemin="280"
                        aria-valuemax={maxSplitWidth()}
                        aria-valuenow={splitWidth}
                        tabindex="0"
                        onpointerdown={onSplitDividerDown}
                        onpointermove={onSplitDividerMove}
                        onpointerup={onSplitDividerUp}
                        onpointercancel={onSplitDividerUp}
                        onkeydown={onSplitDividerKeydown}
                    ></div>
                    <aside class="notes-split" aria-label="Split preview" style={`flex-basis: ${splitWidth}px`}>
                        <header class="notes-split-head">
                            <div class="notes-split-heading">
                                <span class="notes-split-label">Reading alongside</span>
                                {#if splitNote}
                                    <select
                                        class="notes-split-select"
                                        value={splitNote.path}
                                        aria-label="Note shown in split preview"
                                        onchange={(event) => void openSplitPreview(event.currentTarget.value)}
                                    >
                                        {#each $notes as note (note.path)}
                                            <option value={note.path}>{note.title || 'Untitled'}</option>
                                        {/each}
                                    </select>
                                {/if}
                            </div>
                            <div class="notes-split-actions">
                                {#if splitNote}
                                    <button
                                        type="button"
                                        class="notes-context-refresh"
                                        title="Refresh split preview"
                                        aria-label="Refresh split preview"
                                        onclick={() => void openSplitPreview(splitNote!.path)}
                                    >
                                        <RefreshCw />
                                    </button>
                                {/if}
                                <button
                                    type="button"
                                    class="notes-context-close"
                                    title="Close split preview"
                                    aria-label="Close split preview"
                                    onclick={toggleSplit}
                                >
                                    <X />
                                </button>
                            </div>
                        </header>
                        {#if splitLoading}
                            <p class="notes-split-empty">Opening note...</p>
                        {:else if splitNote}
                            <div class="notes-split-reader">
                                <NotesEditor
                                    value={splitNote.body}
                                    noteTitles={getWikiLinkTitles}
                                    resolveNote={resolveNoteTitle}
                                    onNavigateNote={navigateToNote}
                                    editable={false}
                                    showToolbar={false}
                                />
                            </div>
                        {/if}
                    </aside>
                {/if}
            {#if contextOpen}
                <aside class="notes-context" bind:this={contextEl} aria-label="Note context">
                    <header class="notes-context-head">
                        <div class="notes-context-tabs" role="tablist" aria-label="Note sidecar">
                            <button
                                type="button"
                                class="notes-context-tab"
                                class:is-active={contextView === 'context'}
                                role="tab"
                                aria-selected={contextView === 'context'}
                                onclick={() => (contextView = 'context')}
                            >
                                Context
                            </button>
                            <button
                                type="button"
                                class="notes-context-tab"
                                class:is-active={contextView === 'graph'}
                                role="tab"
                                aria-selected={contextView === 'graph'}
                                onclick={() => (contextView = 'graph')}
                            >
                                Graph
                            </button>
                        </div>
                        <button
                            type="button"
                            class="notes-context-close"
                            title="Close note context"
                            aria-label="Close note context"
                            onclick={() => toggleContext(contextView)}
                        >
                            <X />
                        </button>
                    </header>
                    <div class="notes-context-scroll">
                        {#if contextView === 'context'}
                        <section class="notes-context-section">
                            <div class="notes-context-section-head">
                                <div class="notes-context-section-title">
                                    <FileText />
                                    <span>Outline</span>
                                </div>
                                {#if noteOutline.length}
                                    <span class="notes-context-count">{noteOutline.length}</span>
                                {/if}
                            </div>
                            {#if noteOutline.length}
                                <div class="notes-context-list">
                                    {#each noteOutline as heading (heading.index)}
                                        <button
                                            type="button"
                                            class="notes-context-row notes-context-outline"
                                            data-depth={heading.depth}
                                            onclick={() => revealHeading(heading.index)}
                                        >
                                            {heading.text}
                                        </button>
                                    {/each}
                                </div>
                            {:else}
                                <p class="notes-context-empty">Add headings to navigate this note.</p>
                            {/if}
                        </section>

                        <section class="notes-context-section">
                            <div class="notes-context-section-head">
                                <div class="notes-context-section-title">
                                    <Pin />
                                    <span>Links</span>
                                </div>
                                {#if activeLinkContext.length}
                                    <span class="notes-context-count">{activeLinkContext.length}</span>
                                {/if}
                            </div>
                            {#if activeLinkContext.length}
                                <div class="notes-context-list">
                                    {#each activeLinkContext as link (link.title)}
                                        {#if link.matches.length === 1}
                                            <button
                                                type="button"
                                                class="notes-context-row"
                                                onclick={() => void openNote(link.matches[0].path)}
                                            >
                                                <span class="notes-context-row-title">{link.alias || link.title}</span>
                                                <span class="notes-context-row-meta">Open note</span>
                                            </button>
                                        {:else if link.matches.length === 0}
                                            <div class="notes-context-row notes-context-unresolved">
                                                <div>
                                                    <span class="notes-context-row-title">{link.alias || link.title}</span>
                                                    <span class="notes-context-row-meta">No note yet</span>
                                                </div>
                                                <button
                                                    type="button"
                                                    class="notes-context-create"
                                                    onclick={() => createLinkedNote(link.title)}
                                                >
                                                    <Plus /> Create
                                                </button>
                                            </div>
                                        {:else}
                                            <div class="notes-context-row notes-context-ambiguous">
                                                <span class="notes-context-row-title">{link.alias || link.title}</span>
                                                <span class="notes-context-row-meta"
                                                    >{link.matches.length} notes share this title</span
                                                >
                                            </div>
                                        {/if}
                                    {/each}
                                </div>
                            {:else}
                                <p class="notes-context-empty">No note links yet.</p>
                            {/if}
                        </section>

                        <section class="notes-context-section">
                            <div class="notes-context-section-head">
                                <div class="notes-context-section-title">
                                    <RotateCcw />
                                    <span>Linked from</span>
                                </div>
                                <div class="notes-context-section-actions">
                                    {#if backlinks.length}
                                        <span class="notes-context-count">{backlinks.length}</span>
                                    {/if}
                                    <button
                                        type="button"
                                        class="notes-context-refresh"
                                        title="Refresh linked notes"
                                        aria-label="Refresh linked notes"
                                        disabled={contextLoading}
                                        onclick={() => void refreshContextSources()}
                                    >
                                        <RefreshCw
                                            class={`notes-context-refresh-ico${contextLoading ? ' spin' : ''}`}
                                        />
                                    </button>
                                </div>
                            </div>
                            {#if contextError}
                                <p class="notes-context-empty">{contextError}</p>
                            {:else if contextLoading && !backlinks.length}
                                <p class="notes-context-empty">Checking your notes...</p>
                            {:else if backlinks.length}
                                <div class="notes-context-list">
                                    {#each backlinks as backlink (backlink.path)}
                                        <button
                                            type="button"
                                            class="notes-context-row notes-context-backlink"
                                            onclick={() => void openNote(backlink.path)}
                                        >
                                            <span class="notes-context-row-title">{backlink.title}</span>
                                            {#if backlink.preview}
                                                <span class="notes-context-row-preview">{backlink.preview}</span>
                                            {/if}
                                            <span class="notes-context-row-meta"
                                                >{backlink.count} {backlink.count === 1 ? 'link' : 'links'}</span
                                            >
                                        </button>
                                    {/each}
                                </div>
                            {:else}
                                <p class="notes-context-empty">No notes link here yet.</p>
                            {/if}
                        </section>
                        {:else}
                            <section class="notes-graph" aria-label="Current note graph">
                                <p class="notes-graph-kicker">This note's local network</p>
                                {#if contextError}
                                    <p class="notes-context-empty">{contextError}</p>
                                {:else if contextLoading && !graphNodes.length}
                                    <p class="notes-context-empty">Checking linked notes...</p>
                                {:else if graphNodes.length}
                                    <div class="notes-graph-map">
                                        <svg class="notes-graph-lines" viewBox="0 0 100 100" aria-hidden="true">
                                            {#each graphNodes as node (node.path)}
                                                <line x1="50" y1="50" x2={node.x} y2={node.y}></line>
                                            {/each}
                                        </svg>
                                        <div class="notes-graph-center" title={$activeNote.meta.title || 'Untitled'}>
                                            {$activeNote.meta.title || 'Untitled'}
                                        </div>
                                        {#each graphNodes as node (node.path)}
                                            <button
                                                type="button"
                                                class={`notes-graph-node is-${node.direction}`}
                                                style={`left: ${node.x}%; top: ${node.y}%`}
                                                title={`${node.title} (${node.direction})`}
                                                onclick={() => void openNote(node.path)}
                                            >
                                                {node.title}
                                            </button>
                                        {/each}
                                    </div>
                                    <p class="notes-graph-legend">
                                        Incoming notes point here. Outgoing notes are linked from here.
                                    </p>
                                {:else}
                                    <p class="notes-context-empty">
                                        Add a <code>[[note link]]</code> to start this note's graph.
                                    </p>
                                {/if}
                                <button
                                    type="button"
                                    class="notes-graph-refresh"
                                    disabled={contextLoading}
                                    onclick={() => void refreshContextSources()}
                                >
                                    <RefreshCw class={contextLoading ? 'notes-context-refresh-ico spin' : ''} />
                                    Refresh links
                                </button>
                            </section>
                        {/if}
                    </div>
                </aside>
            {/if}
            </div>
        {:else}
            <div class="notes-empty">
                <div class="notes-empty-ico" aria-hidden="true"><FileText /></div>
                <h2 class="notes-empty-title">Your local notebook</h2>
                <p class="notes-empty-desc">
                    Notes are plain <code>.ki</code> files in
                    <strong>Documents\KeepItLocal&nbsp;Notes</strong> — Markdown, yours forever, found
                    by Search. Pick a note on the left, or create one.
                </p>
                <button type="button" class="notes-new notes-empty-new" onclick={() => void createNote()}>
                    <Plus class="notes-new-ico" /> New note
                </button>
            </div>
        {/if}
    </section>
</div>

<dialog class="notes-history" bind:this={revisionDialog} aria-labelledby="notes-history-title">
    <section class="notes-history-panel">
        <header class="notes-history-head">
            <div>
                <p class="notes-capture-kicker">Local recovery</p>
                <h2 id="notes-history-title">Version history</h2>
            </div>
            <button
                type="button"
                class="notes-context-close"
                aria-label="Close version history"
                onclick={() => revisionDialog?.close()}
            >
                <X />
            </button>
        </header>
        <p class="notes-history-intro">Every saved edit keeps the previous version on this device.</p>
        {#if revisionsLoading}
            <p class="notes-history-empty">Loading saved versions...</p>
        {:else if revisionError && !selectedRevision}
            <p class="notes-history-empty">Could not load version history: {revisionError}</p>
        {:else if !revisions.length}
            <p class="notes-history-empty">No earlier versions yet. Save another edit to create one.</p>
        {:else}
            <div class="notes-history-layout">
                <div class="notes-history-list" aria-label="Saved versions">
                    {#each revisions as revision (revision.id)}
                        <button
                            type="button"
                            class="notes-history-item"
                            class:is-active={selectedRevision?.id === revision.id}
                            onclick={() => void selectRevision(revision)}
                        >
                            <span>{relTime(revision.createdMs)}</span>
                            <small>{new Date(revision.createdMs).toLocaleString()}</small>
                        </button>
                    {/each}
                </div>
                <section class="notes-history-detail" aria-live="polite">
                    {#if selectedRevision}
                        <div class="notes-history-detail-head">
                            <div>
                                <strong>Changes since {relTime(selectedRevision.createdMs)}</strong>
                                <span>Your current saved note is on the right side of the diff.</span>
                            </div>
                            <button
                                type="button"
                                class="notes-history-restore"
                                disabled={revisionRestoring}
                                onclick={() => void restoreSelectedRevision()}
                            >
                                <RotateCcw /> {revisionRestoring ? 'Restoring...' : 'Restore this version'}
                            </button>
                        </div>
                        {#if revisionLoading}
                            <p class="notes-history-empty">Comparing versions...</p>
                        {:else if revisionError}
                            <p class="notes-history-empty">Could not compare versions: {revisionError}</p>
                        {:else if revisionDiff}
                            <pre class="notes-history-diff">{#each revisionDiff.split('\n') as line}<span class:diff-add={diffLineKind(line) === 'add'} class:diff-remove={diffLineKind(line) === 'remove'} class:diff-hunk={diffLineKind(line) === 'hunk'} class:diff-file={diffLineKind(line) === 'file'}>{line || ' '}</span>{'\n'}{/each}</pre>
                        {:else}
                            <p class="notes-history-empty">This version matches the current note.</p>
                        {/if}
                    {/if}
                </section>
            </div>
        {/if}
    </section>
</dialog>

<dialog class="notes-capture" bind:this={captureDialog} aria-labelledby="notes-capture-title">
    <form
        class="notes-capture-form"
        onsubmit={(event) => {
            event.preventDefault();
            void saveCapture();
        }}
    >
        <header class="notes-capture-head">
            <div>
                <p class="notes-capture-kicker">Inbox capture</p>
                <h2 id="notes-capture-title">Save research locally</h2>
            </div>
            <button type="button" class="notes-context-close" aria-label="Close capture" onclick={() => captureDialog?.close()}>
                <X />
            </button>
        </header>
        <label class="notes-capture-field">
            <span>Title <em>optional</em></span>
            <input bind:value={captureTitle} placeholder="Research note" />
        </label>
        <label class="notes-capture-field">
            <span>Source <em>optional URL</em></span>
            <input bind:value={captureSource} placeholder="https://example.com/article" inputmode="url" />
        </label>
        <label class="notes-capture-field notes-capture-body">
            <span>Content</span>
            <textarea bind:value={captureBody} placeholder="Paste notes, an article excerpt, or import Markdown..." required></textarea>
        </label>
        <div class="notes-capture-tools">
            <button type="button" class="notes-capture-link" onclick={() => void pasteCapture()}>Paste clipboard</button>
            <button type="button" class="notes-capture-link" onclick={() => void importCapture()}>Import Markdown/text</button>
        </div>
        <footer class="notes-capture-actions">
            <button type="button" class="notes-capture-cancel" onclick={() => captureDialog?.close()}>Cancel</button>
            <button type="submit" class="notes-capture-save" disabled={captureSaving}>
                {captureSaving ? 'Saving...' : 'Save to Inbox'}
            </button>
        </footer>
    </form>
</dialog>

<style>
    /* Make the page host fill the viewport so the two panes can scroll
       independently (the app's <main> has a definite height; tool-host is
       content-height by default — scoped to Notes only). */
    :global(.tool-host[data-screen='notes']) {
        height: 100%;
    }

    .notes {
        display: flex;
        height: 100%;
        min-height: 0;
        background: var(--color-bg);
    }
    .notes.is-focus .notes-list {
        display: none;
    }

    /* ─── List pane ─────────────────────────────────────────────── */
    .notes-list {
        flex: none;
        width: 316px;
        display: flex;
        flex-direction: column;
        min-height: 0;
        border-right: 1px solid var(--color-border);
    }
    .notes-list-head {
        flex: none;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
        padding: 14px 14px 10px;
    }
    .notes-brand {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        font-size: 15px;
        font-weight: 700;
        color: var(--color-text);
    }
    .notes-list :global(.notes-brand-ico) {
        width: 17px;
        height: 17px;
        color: var(--color-accent);
    }
    .notes-head-actions {
        display: inline-flex;
        align-items: center;
        gap: 8px;
    }
    .notes-new-group {
        display: inline-flex;
        align-items: stretch;
    }
    .notes-tpl {
        position: relative;
        display: inline-flex;
    }
    .notes-tpl-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 26px;
        height: 30px;
        border-radius: 0 8px 8px 0;
        border: 1px solid color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
        color: var(--color-accent);
        cursor: pointer;
    }
    .notes-tpl-btn:hover {
        background: color-mix(in srgb, var(--color-accent) 22%, transparent);
    }
    .notes-tpl :global(.notes-tpl-ico) {
        width: 14px;
        height: 14px;
    }
    .notes-tpl-menu {
        position: absolute;
        top: calc(100% + 4px);
        right: 0;
        z-index: 20;
        min-width: 170px;
        max-height: 260px;
        overflow-y: auto;
        padding: 4px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 10px;
        box-shadow: var(--shadow-md, 0 8px 24px rgb(0 0 0 / 0.28));
    }
    .notes-tpl-opt {
        display: block;
        width: 100%;
        padding: 6px 10px;
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
    .notes-tpl-opt:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .notes-tpl-manage {
        margin-top: 2px;
        padding-top: 7px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
        border-radius: 0 0 6px 6px;
        color: var(--color-muted);
    }
    .notes-trash {
        display: flex;
        flex-direction: column;
        gap: 2px;
        padding: 2px 0;
    }
    .notes-trash-msg {
        margin: 0;
        padding: 12px 4px;
        color: var(--color-muted);
        font-size: 12.5px;
        line-height: 1.5;
    }
    .notes-trash-row {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 9px 8px;
        border-radius: 8px;
    }
    .notes-trash-row:hover {
        background: var(--color-panel);
    }
    .notes-trash-name {
        flex: 1;
        min-width: 0;
        display: flex;
        align-items: center;
        gap: 6px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 13px;
        color: var(--color-text);
    }
    .notes-trash-kind {
        flex: none;
        width: 14px;
        height: 14px;
        color: var(--color-muted);
    }
    .notes-trash-when {
        flex: none;
        color: var(--color-muted);
        font-size: 10.5px;
        font-variant-numeric: tabular-nums;
    }
    .notes-trash-act {
        flex: none;
        display: inline-flex;
        padding: 4px;
        border: none;
        border-radius: 6px;
        background: none;
        color: var(--color-text-secondary);
        cursor: pointer;
    }
    .notes-trash-act:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .notes-trash-act-danger:hover {
        color: var(--color-error);
    }
    .notes-trash :global(.notes-trash-act-ico) {
        width: 14px;
        height: 14px;
    }

    .notes-more {
        position: relative;
    }
    .notes-more-menu {
        position: absolute;
        top: calc(100% + 6px);
        right: 0;
        z-index: 30;
        width: 210px;
        max-height: min(420px, calc(100vh - 120px));
        overflow-y: auto;
        padding: 5px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 10px;
        box-shadow: var(--shadow-md, 0 10px 32px rgb(0 0 0 / 0.28));
    }
    .notes-more-label {
        margin: 6px 8px 4px;
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.06em;
        text-transform: uppercase;
    }
    .notes-more-option {
        display: flex;
        align-items: center;
        gap: 8px;
        width: 100%;
        min-height: 30px;
        padding: 5px 8px;
        border: none;
        border-radius: 6px;
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12.5px;
        text-align: left;
        cursor: pointer;
    }
    .notes-more-option:hover:not(:disabled) {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .notes-more-option:disabled {
        color: var(--color-muted);
        cursor: default;
    }
    .notes-more-option .notes-export-ext {
        margin-left: auto;
    }
    .notes-more-delete {
        color: var(--color-error);
    }
    .notes-more-divider {
        height: 1px;
        margin: 5px 3px;
        background: var(--color-border);
    }
    .notes-more :global(.notes-more-ico) {
        width: 14px;
        height: 14px;
        flex: none;
    }
    .notes-export-ext {
        color: var(--color-muted);
        font-family: var(--font-mono, monospace);
        font-size: 10.5px;
    }

    /* Shown only with zero templates — the menu has to teach the feature,
       since it's also the only way to create the folder. */
    .notes-tpl-hint {
        max-width: 190px;
        margin: 2px 0 0;
        padding: 4px 10px 6px;
        color: var(--color-muted);
        font-size: 11px;
        line-height: 1.45;
    }
    .notes-tpl-hint code {
        font-family: var(--font-mono, monospace);
        font-size: 10px;
    }
    .notes-new {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        height: 30px;
        padding: 0 11px;
        border-radius: 8px 0 0 8px;
        border: 1px solid color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
        border-right: none;
        color: var(--color-accent);
        font-size: 12.5px;
        font-weight: 600;
        cursor: pointer;
    }
    .notes-new:hover {
        background: color-mix(in srgb, var(--color-accent) 22%, transparent);
    }
    .notes-list :global(.notes-new-ico) {
        width: 14px;
        height: 14px;
    }
    .notes-icon-btn {
        display: grid;
        place-items: center;
        width: 30px;
        height: 30px;
        border-radius: 8px;
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
        cursor: pointer;
    }
    .notes-icon-btn:hover {
        color: var(--color-text);
        border-color: var(--color-border-strong, var(--color-border));
    }
    .notes-icon-btn.is-on {
        color: var(--color-accent);
        border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
        background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    }
    :global(.notes-icon-btn-ico) {
        width: 15px;
        height: 15px;
    }

    .notes-search {
        flex: none;
        position: relative;
        margin: 0 12px 8px;
    }
    .notes-list :global(.notes-search-ico) {
        position: absolute;
        left: 10px;
        top: 50%;
        transform: translateY(-50%);
        width: 14px;
        height: 14px;
        color: var(--color-muted);
        pointer-events: none;
    }
    .notes-search-input {
        width: 100%;
        height: 34px;
        padding: 0 10px 0 32px;
        border-radius: 8px;
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-size: 13px;
    }
    .notes-search-input:focus {
        outline: none;
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
    }

    .notes-library {
        flex: none;
        padding: 2px 8px 10px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .notes-library:not([open]) {
        padding-bottom: 2px;
    }
    .notes-library-toggle {
        display: flex;
        align-items: center;
        justify-content: space-between;
        min-height: 32px;
        padding: 5px 8px;
        border-radius: 7px;
        color: var(--color-text-secondary);
        cursor: pointer;
        list-style: none;
        user-select: none;
    }
    .notes-library-toggle::-webkit-details-marker {
        display: none;
    }
    .notes-library-toggle::marker {
        content: '';
    }
    .notes-library-toggle:hover {
        background: var(--color-panel);
        color: var(--color-text);
    }
    .notes-library-toggle:focus-visible {
        outline: 2px solid color-mix(in srgb, var(--color-accent) 55%, transparent);
        outline-offset: -2px;
    }
    .notes-library-toggle-title {
        margin: 0;
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.06em;
        text-transform: uppercase;
        display: inline-flex;
        align-items: center;
        gap: 8px;
    }
    .notes-library :global(.notes-library-toggle-ico),
    .notes-library :global(.notes-library-toggle-chevron) {
        width: 14px;
        height: 14px;
    }
    .notes-library[open] :global(.notes-library-toggle-chevron) {
        transform: rotate(180deg);
    }
    .notes-library-content {
        display: flex;
        flex-direction: column;
        gap: 12px;
        max-height: min(308px, 38vh);
        overflow-y: auto;
        padding-top: 4px;
    }
    .notes-library-section {
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
    .notes-library-section-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        min-height: 23px;
        padding: 0 8px;
    }
    .notes-library-label {
        margin: 0;
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.07em;
        text-transform: uppercase;
    }
    .notes-library-item {
        position: relative;
        display: flex;
        align-items: center;
        gap: 8px;
        width: 100%;
        min-height: 29px;
        padding: 5px 8px;
        border: none;
        border-radius: 7px;
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12.5px;
        text-align: left;
        cursor: pointer;
    }
    .notes-library-item:hover {
        background: var(--color-panel);
        color: var(--color-text);
    }
    .notes-library-item.is-active {
        background: var(--color-panel-2);
        color: var(--color-accent);
    }
    .notes-library-item.is-active::before {
        content: '';
        position: absolute;
        top: 6px;
        bottom: 6px;
        left: 2px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .notes-library-today {
        color: color-mix(in srgb, var(--color-accent) 78%, var(--color-text-secondary));
        font-weight: 600;
    }
    .notes-library-today:hover {
        color: var(--color-accent);
    }
    .notes-library :global(.notes-library-ico) {
        width: 14px;
        height: 14px;
        flex: none;
    }
    .notes-library :global(.notes-library-section-ico) {
        width: 13px;
        height: 13px;
        color: var(--color-muted);
    }
    .notes-library-name {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .notes-library-count {
        margin-left: auto;
        color: var(--color-muted);
        font-size: 10.5px;
        font-variant-numeric: tabular-nums;
    }
    .notes-library-item.is-active .notes-library-count {
        color: color-mix(in srgb, var(--color-accent) 72%, var(--color-muted));
    }
    .notes-library-add {
        display: grid;
        place-items: center;
        width: 23px;
        height: 23px;
        padding: 0;
        border: none;
        border-radius: 6px;
        background: transparent;
        color: var(--color-muted);
        cursor: pointer;
    }
    .notes-library-add:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .notes-library-input {
        width: calc(100% - 16px);
        height: 29px;
        margin: 2px 8px;
        padding: 0 8px;
        border: 1px solid color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        border-radius: 7px;
        outline: none;
        background: var(--color-panel-2);
        color: var(--color-text);
        font-size: 12px;
    }
    .notes-library-hint {
        margin: 2px 8px 0;
        color: var(--color-muted);
        font-size: 11.5px;
        line-height: 1.45;
    }
    .notes-folder-row {
        position: relative;
        display: flex;
        align-items: center;
    }
    .notes-folder-row .notes-library-item {
        padding-right: 58px;
    }
    .notes-folder-row:hover .notes-library-count,
    .notes-folder-row:focus-within .notes-library-count {
        opacity: 0;
    }
    .notes-folder-hover-actions {
        position: absolute;
        right: 7px;
        display: flex;
        gap: 2px;
        opacity: 0;
        pointer-events: none;
        transition: opacity var(--dur-micro) var(--ease-out);
    }
    .notes-folder-row:hover .notes-folder-hover-actions,
    .notes-folder-row:focus-within .notes-folder-hover-actions {
        opacity: 1;
        pointer-events: auto;
    }
    .notes-folder-hover-action {
        display: grid;
        place-items: center;
        width: 22px;
        height: 22px;
        padding: 0;
        border: none;
        border-radius: 5px;
        background: var(--color-panel);
        color: var(--color-muted);
        cursor: pointer;
    }
    .notes-folder-hover-action:hover,
    .notes-folder-hover-action:focus-visible {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .notes-folder-hover-action.is-danger:hover,
    .notes-folder-hover-action.is-danger:focus-visible {
        color: var(--color-error);
    }
    .notes-folder-hover-action :global(svg) {
        width: 12px;
        height: 12px;
    }
    .notes-folder-rename-input {
        margin-block: 0;
    }
    .notes-library-tag-mark {
        color: var(--color-muted);
        font-family: var(--font-mono, monospace);
        font-size: 12px;
    }
    .notes-library-item.is-active .notes-library-tag-mark {
        color: var(--color-accent);
    }
    .notes-library-divider {
        height: 1px;
        margin: -2px 8px 0;
        background: var(--color-border);
    }
    .notes-library-trash:hover {
        color: var(--color-error);
    }
    .notes-library-trash.is-active {
        color: var(--color-error);
    }
    .notes-library-trash.is-active::before {
        background: var(--color-error);
    }

    .notes-scroll {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        padding: 0px 8px 16px;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
    .notes-results-head {
        position: sticky;
        top: 0;
        z-index: 2;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
        min-height: 44px;
        padding: 6px 8px 8px;
        background: var(--color-bg);
    }
    .notes-results-head h2 {
        margin: 1px 0 0;
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.06em;
        text-transform: uppercase;
    }
    .notes-results-kicker {
        margin: 0;
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.06em;
        text-transform: uppercase;
    }
    .notes-results-count {
        flex: none;
        color: var(--color-muted);
        font-size: 11px;
        font-variant-numeric: tabular-nums;
        white-space: nowrap;
    }
    .notes-results-summary {
        display: flex;
        align-items: center;
        gap: 7px;
    }
    .notes-results-keyhint {
        display: inline-flex;
        align-items: center;
        gap: 3px;
        color: var(--color-muted);
    }
    .notes-results-keyhint kbd {
        min-width: 18px;
        padding: 1px 3px;
        border: 1px solid color-mix(in srgb, var(--color-text) 10%, transparent);
        border-radius: 4px;
        color: var(--color-text-secondary);
        font-family: var(--font-mono, monospace);
        font-size: 9px;
        line-height: 1.2;
        text-align: center;
    }
    .notes-text-action {
        flex: none;
        padding: 3px 0;
        border: none;
        background: transparent;
        color: var(--color-error);
        font-size: 11.5px;
        font-weight: 600;
        cursor: pointer;
    }
    .notes-text-action:hover {
        text-decoration: underline;
    }
    .notes-msg {
        padding: 18px 12px;
        font-size: 13px;
        color: var(--color-text-secondary);
    }

    .notes-row-wrap {
        position: relative;
    }
    .notes-row {
        position: relative;
        display: flex;
        align-items: flex-start;
        width: 100%;
        text-align: left;
        padding: 10px 42px 10px 12px;
        border: none;
        border-radius: 8px;
        background: transparent;
        cursor: pointer;
    }
    .notes-row:hover {
        background: var(--color-panel);
    }
    .notes-row.is-active {
        background: var(--color-panel-2);
    }
    .notes-row-wrap.is-search-selected .notes-row:not(.is-active) {
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-accent) 48%, transparent);
    }
    .notes-row-wrap.is-active .notes-row::before {
        content: '';
        position: absolute;
        left: 3px;
        top: 9px;
        bottom: 9px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .notes-row-main {
        flex: 1;
        min-width: 0;
    }
    .notes-row-title {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: 13.5px;
        font-weight: 600;
        color: var(--color-text);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .notes-row-wrap.is-active .notes-row-title {
        color: var(--color-accent);
    }
    .notes-row :global(.notes-row-pin) {
        width: 11px;
        height: 11px;
        flex: none;
        color: var(--color-accent);
    }
    .notes-row-preview {
        margin-top: 2px;
        font-size: 12px;
        color: var(--color-text-secondary);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .notes-row-match {
        display: inline-flex;
        margin-top: 5px;
        color: var(--color-accent);
        font-size: 10.5px;
        font-weight: 600;
    }
    .notes-row-tags {
        display: flex;
        flex-wrap: wrap;
        gap: 4px;
        margin-top: 4px;
    }
    /* Same chip formula as the editor's tag input and the palette's note
       preview, so a tag looks like a tag everywhere in the app. */
    .notes-row-tag {
        display: inline-flex;
        padding: 1px 6px;
        background: color-mix(in srgb, var(--color-accent) 9%, transparent);
        color: color-mix(in srgb, var(--color-accent) 80%, var(--color-text));
        border-radius: 5px;
        font-size: 9.5px;
        font-weight: 500;
    }
    .notes-row-tag-more {
        display: inline-flex;
        padding: 1px 4px;
        color: var(--color-muted);
        font-size: 9.5px;
        font-weight: 500;
        font-variant-numeric: tabular-nums;
    }
    .notes-row-meta {
        margin-top: 3px;
        font-size: 11px;
        color: var(--color-muted);
    }
    .notes-row-act {
        position: absolute;
        top: 8px;
        right: 6px;
        display: grid;
        place-items: center;
        width: 24px;
        height: 24px;
        padding: 0;
        border: none;
        border-radius: 6px;
        background: transparent;
        color: var(--color-muted);
        opacity: 0;
        cursor: pointer;
        transition: opacity 120ms ease, color 120ms ease;
    }
    .notes-row-wrap:hover .notes-row-act,
    .notes-row-act:focus-visible,
    .notes-row-act.is-on {
        opacity: 1;
    }
    .notes-row-act.is-on {
        color: var(--color-accent);
    }
    .notes-row-act:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .notes-row-act :global(.notes-row-act-ico) {
        width: 13px;
        height: 13px;
    }

    /* ─── Editor pane ───────────────────────────────────────────── */
    .notes-editor {
        flex: 1;
        min-width: 0;
        position: relative;
        display: flex;
        flex-direction: column;
        min-height: 0;
    }
    .notes-editor-head {
        flex: none;
        display: flex;
        align-items: flex-start;
        gap: 16px;
        padding: 16px clamp(20px, 4vw, 48px) 9px;
    }
    .notes-editor-heading {
        flex: 1;
        min-width: 0;
    }
    .notes-breadcrumb {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        min-height: 16px;
        color: var(--color-muted);
        font-size: 12px;
        line-height: 1;
    }
    .notes-breadcrumb :global(.notes-breadcrumb-ico) {
        width: 12px;
        height: 12px;
    }
    .notes-title {
        padding-left: 5px;
        border: none;
        background: transparent;
        color: var(--color-text);
        font-size: clamp(20px, 2vw, 25px);
        font-weight: 700;
        letter-spacing: -0.025em;
        line-height: 1.15;
    }
    .notes-title:focus {
        outline: none;
    }
    .notes-title::placeholder {
        color: var(--color-muted);
    }
    .notes-save-status {
        flex: none;
        display: inline-flex;
        align-items: center;
        gap: 4px;
        font-size: 11.5px;
        color: var(--color-muted);
        white-space: nowrap;
    }
    .notes-save-status.is-saved {
        color: var(--color-accent);
    }
    .notes-save-status :global(.notes-save-ico) {
        width: 12px;
        height: 12px;
    }
    .notes-editor-actions {
        flex: none;
        display: inline-flex;
        align-items: center;
        gap: 6px;
    }
    .notes-external-change {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 16px;
        margin: 0 clamp(20px, 4vw, 48px);
        padding: 10px 0;
        border-block: 1px solid color-mix(in srgb, var(--color-error, #b5352c) 26%, var(--color-border));
    }
    .notes-external-change-copy {
        display: grid;
        gap: 2px;
        min-width: 0;
        font-size: 12px;
        line-height: 1.4;
    }
    .notes-external-change-copy strong {
        color: var(--color-error, #b5352c);
        font-size: 12.5px;
    }
    .notes-external-change-copy span {
        color: var(--color-muted);
    }
    .notes-external-change-actions {
        flex: none;
        display: inline-flex;
        align-items: center;
        gap: 6px;
    }
    .notes-external-change-btn {
        min-height: 28px;
        padding: 4px 9px;
        border: 1px solid var(--color-border);
        border-radius: 7px;
        background: transparent;
        color: var(--color-text-secondary);
        font: inherit;
        font-size: 11.5px;
        font-weight: 600;
        cursor: pointer;
    }
    .notes-external-change-btn:hover {
        border-color: var(--color-text-muted, var(--color-border));
        color: var(--color-text);
    }
    .notes-external-change-btn:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: 2px;
    }
    .notes-external-change-btn.is-primary {
        border-color: color-mix(in srgb, var(--color-accent) 48%, var(--color-border));
        color: var(--color-accent);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .notes-tags-row {
        display: flex;
        align-items: center;
        gap: 8px;
        min-height: 34px;
        padding: 0 clamp(20px, 4vw, 48px) 9px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .notes-tags-label {
        flex: none;
        color: var(--color-muted);
        font-size: 11px;
        font-weight: 600;
    }
    .notes-tags-row :global(.tag-input) {
        flex: 1;
        min-width: 0;
    }
    .notes-editor-body {
        flex: 1;
        min-height: 0;
        min-width: 0;
        display: flex;
        flex-direction: column;
    }
    .notes-workspace {
        position: relative;
        flex: 1;
        display: flex;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
    }
    .notes-editor-body :global(.ne) {
        flex: 1;
        min-height: 0;
    }
    .notes-editor-body :global(.ne-toolbar) {
        padding: 7px clamp(20px, 4vw, 48px);
        border-bottom-color: var(--color-divider, var(--color-border));
    }
    .notes-editor-body :global(.ne-surface .tiptap) {
        box-sizing: border-box;
        margin-inline: auto;
        padding: 30px clamp(20px, 4vw, 48px) 96px;
    }
    .notes-split-divider {
        position: relative;
        z-index: 1;
        flex: 0 0 9px;
        cursor: col-resize;
        touch-action: none;
    }
    .notes-split-divider::after {
        content: '';
        position: absolute;
        top: 0;
        bottom: 0;
        left: 4px;
        width: 1px;
        background: var(--color-border);
        transition: background-color var(--dur-micro) var(--ease-out), width var(--dur-micro) var(--ease-out);
    }
    .notes-split-divider:hover::after,
    .notes-split-divider.is-dragging::after,
    .notes-split-divider:focus-visible::after {
        left: 3px;
        width: 3px;
        background: var(--color-accent);
    }
    .notes-split-divider:focus-visible {
        outline: none;
    }
    .notes-split {
        flex: 0 0 360px;
        min-width: 280px;
        display: flex;
        flex-direction: column;
        min-height: 0;
        border-left: 1px solid var(--color-border);
        background: var(--color-bg);
    }
    .notes-split-head {
        flex: none;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        min-height: 52px;
        padding: 0 10px 0 14px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .notes-split-heading {
        min-width: 0;
    }
    .notes-split-label {
        display: block;
        margin-bottom: 2px;
        color: var(--color-muted);
        font-size: 9.5px;
        font-weight: 700;
        letter-spacing: 0.07em;
        text-transform: uppercase;
    }
    .notes-split-select {
        width: 100%;
        max-width: 230px;
        padding: 0;
        border: none;
        background: transparent;
        color: var(--color-text);
        font: inherit;
        font-size: 12px;
        font-weight: 650;
        text-overflow: ellipsis;
    }
    .notes-split-select:focus-visible {
        outline: 2px solid color-mix(in srgb, var(--color-accent) 55%, transparent);
        outline-offset: 3px;
    }
    .notes-split-actions {
        flex: none;
        display: flex;
        gap: 3px;
    }
    .notes-split-reader {
        flex: 1;
        min-height: 0;
    }
    .notes-split-reader :global(.ne-surface .tiptap) {
        padding: 20px 20px 64px;
        font-size: 13.5px;
    }
    .notes-split-empty {
        margin: 18px;
        color: var(--color-muted);
        font-size: 12px;
    }

    /* ─── Empty state ───────────────────────────────────────────── */
    .notes-context {
        position: relative;
        flex: 0 1 286px;
        display: flex;
        flex-direction: column;
        width: 286px;
        min-width: 0;
        background: var(--color-bg);
        border-left: 1px solid var(--color-border);
    }
    .notes-context-head {
        flex: none;
        display: flex;
        align-items: center;
        justify-content: space-between;
        min-height: 52px;
        padding: 0 12px 0 16px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        color: var(--color-text);
        font-size: 12px;
        font-weight: 700;
    }
    .notes-context-tabs {
        display: inline-flex;
        align-items: center;
        gap: 3px;
    }
    .notes-context-tab {
        padding: 4px 3px;
        border: none;
        border-bottom: 1px solid transparent;
        background: transparent;
        color: var(--color-muted);
        font: inherit;
        font-size: 12px;
        font-weight: 650;
        cursor: pointer;
    }
    .notes-context-tab + .notes-context-tab {
        margin-left: 8px;
    }
    .notes-context-tab.is-active {
        border-bottom-color: var(--color-accent);
        color: var(--color-text);
    }
    .notes-context-close,
    .notes-context-refresh {
        display: grid;
        place-items: center;
        width: 28px;
        height: 28px;
        padding: 0;
        border: none;
        border-radius: 6px;
        background: transparent;
        color: var(--color-muted);
        cursor: pointer;
    }
    .notes-context-close:hover,
    .notes-context-refresh:hover:not(:disabled) {
        background: var(--color-panel);
        color: var(--color-text);
    }
    .notes-context-close:focus-visible,
    .notes-context-refresh:focus-visible,
    .notes-context-row:focus-visible,
    .notes-context-create:focus-visible {
        outline: 2px solid color-mix(in srgb, var(--color-accent) 55%, transparent);
        outline-offset: -2px;
    }
    .notes-context-refresh:disabled {
        cursor: default;
    }
    .notes-context-close :global(svg),
    .notes-context-refresh :global(svg) {
        width: 15px;
        height: 15px;
    }
    .notes-context-scroll {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
    }
    .notes-context-section {
        padding: 14px 12px 15px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .notes-context-section:last-child {
        border-bottom: none;
    }
    .notes-context-section-head,
    .notes-context-section-title,
    .notes-context-section-actions {
        display: flex;
        align-items: center;
    }
    .notes-context-section-head {
        justify-content: space-between;
        gap: 8px;
        min-height: 22px;
        margin-bottom: 6px;
    }
    .notes-context-section-title {
        gap: 6px;
        min-width: 0;
        color: var(--color-text-secondary);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.06em;
        text-transform: uppercase;
    }
    .notes-context-section-title :global(svg) {
        width: 13px;
        height: 13px;
        color: var(--color-muted);
    }
    .notes-context-section-actions {
        gap: 4px;
    }
    .notes-context-count {
        color: var(--color-muted);
        font-size: 10.5px;
        font-variant-numeric: tabular-nums;
    }
    .notes-context-list {
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
    .notes-context-row {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        width: 100%;
        min-width: 0;
        padding: 7px 8px;
        border: none;
        border-radius: 7px;
        background: transparent;
        color: var(--color-text-secondary);
        font-family: inherit;
        font-size: 12px;
        line-height: 1.35;
        text-align: left;
    }
    button.notes-context-row {
        cursor: pointer;
    }
    button.notes-context-row:hover {
        background: var(--color-panel);
        color: var(--color-text);
    }
    .notes-context-row-title,
    .notes-context-row-meta,
    .notes-context-row-preview {
        display: block;
        max-width: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .notes-context-row-title {
        color: var(--color-text);
        font-weight: 600;
    }
    .notes-context-row-meta {
        margin-top: 2px;
        color: var(--color-muted);
        font-size: 10.5px;
    }
    .notes-context-row-preview {
        margin-top: 2px;
        color: var(--color-text-secondary);
        font-size: 11px;
    }
    .notes-context-outline {
        color: var(--color-text-secondary);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .notes-context-outline[data-depth='2'] { padding-left: 18px; }
    .notes-context-outline[data-depth='3'] { padding-left: 28px; }
    .notes-context-outline[data-depth='4'] { padding-left: 38px; }
    .notes-context-outline[data-depth='5'] { padding-left: 48px; }
    .notes-context-outline[data-depth='6'] { padding-left: 58px; }
    .notes-context-unresolved {
        flex-direction: row;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
    }
    .notes-context-unresolved > div {
        min-width: 0;
    }
    .notes-context-create {
        flex: none;
        display: inline-flex;
        align-items: center;
        gap: 4px;
        padding: 3px 2px;
        border: none;
        background: transparent;
        color: var(--color-accent);
        font-size: 10.5px;
        font-weight: 700;
        cursor: pointer;
    }
    .notes-context-create:hover {
        text-decoration: underline;
    }
    .notes-context-create :global(svg) {
        width: 12px;
        height: 12px;
    }
    .notes-context-ambiguous .notes-context-row-meta {
        color: var(--color-warning, var(--color-muted));
    }
    .notes-context-empty {
        margin: 3px 8px 1px;
        color: var(--color-muted);
        font-size: 11.5px;
        line-height: 1.45;
    }
    :global(.notes-context-refresh-ico.spin) {
        animation: notes-context-spin 800ms linear infinite;
    }
    @keyframes notes-context-spin {
        to { transform: rotate(360deg); }
    }
    @media (prefers-reduced-motion: reduce) {
        :global(.notes-context-refresh-ico.spin) { animation: none; }
    }

    .notes-graph {
        padding: 16px 12px;
    }
    .notes-graph-kicker {
        margin: 0 4px 12px;
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.06em;
        text-transform: uppercase;
    }
    .notes-graph-map {
        position: relative;
        aspect-ratio: 1;
        min-height: 230px;
    }
    .notes-graph-lines {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 100%;
        overflow: visible;
    }
    .notes-graph-lines line {
        stroke: color-mix(in srgb, var(--color-text) 16%, transparent);
        stroke-width: 0.7;
        vector-effect: non-scaling-stroke;
    }
    .notes-graph-center,
    .notes-graph-node {
        position: absolute;
        transform: translate(-50%, -50%);
        max-width: 96px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        text-align: center;
    }
    .notes-graph-center {
        left: 50%;
        top: 50%;
        padding: 7px 9px;
        border: 1px solid color-mix(in srgb, var(--color-accent) 48%, var(--color-border));
        border-radius: 7px;
        color: var(--color-accent);
        font-size: 11px;
        font-weight: 700;
    }
    .notes-graph-node {
        padding: 5px 7px;
        border: 1px solid var(--color-border);
        border-radius: 6px;
        background: var(--color-bg);
        color: var(--color-text-secondary);
        font: inherit;
        font-size: 10.5px;
        cursor: pointer;
    }
    .notes-graph-node:hover,
    .notes-graph-node:focus-visible {
        border-color: color-mix(in srgb, var(--color-accent) 48%, var(--color-border));
        color: var(--color-text);
        outline: none;
    }
    .notes-graph-node.is-incoming { border-left-color: var(--color-text-secondary); }
    .notes-graph-node.is-outgoing { border-left-color: var(--color-accent); }
    .notes-graph-node.is-both { border-left-color: var(--color-success, var(--color-accent)); }
    .notes-graph-legend {
        margin: 10px 4px 0;
        color: var(--color-muted);
        font-size: 10.5px;
        line-height: 1.45;
    }
    .notes-graph-refresh {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        margin: 14px 4px 0;
        padding: 3px 0;
        border: none;
        background: transparent;
        color: var(--color-text-secondary);
        font: inherit;
        font-size: 11px;
        font-weight: 650;
        cursor: pointer;
    }
    .notes-graph-refresh:hover:not(:disabled) { color: var(--color-accent); }
    .notes-graph-refresh:disabled { cursor: default; }
    .notes-graph-refresh :global(svg) { width: 13px; height: 13px; }

    .notes-history {
        width: min(920px, calc(100vw - 32px));
        max-height: min(720px, calc(100vh - 32px));
        padding: 0;
        overflow: hidden;
        border: 1px solid var(--color-border);
        border-radius: 12px;
        background: var(--color-bg);
        color: var(--color-text);
        box-shadow: var(--shadow-lg, 0 24px 70px rgb(0 0 0 / 0.34));
    }
    .notes-history::backdrop { background: color-mix(in srgb, var(--color-text) 34%, transparent); }
    .notes-history-panel { display: grid; gap: 12px; padding: 18px; }
    .notes-history-head {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 16px;
    }
    .notes-history-head h2 {
        margin: 0;
        color: var(--color-text);
        font-size: 17px;
        letter-spacing: -0.02em;
    }
    .notes-history-intro {
        margin: -5px 0 0;
        color: var(--color-text-secondary);
        font-size: 12px;
        line-height: 1.45;
    }
    .notes-history-layout {
        display: grid;
        grid-template-columns: minmax(160px, 0.34fr) minmax(0, 1fr);
        min-height: 340px;
        max-height: min(510px, calc(100vh - 210px));
        border: 1px solid var(--color-border);
        border-radius: 8px;
        overflow: hidden;
    }
    .notes-history-list {
        min-width: 0;
        overflow: auto;
        border-right: 1px solid var(--color-border);
        padding: 5px;
    }
    .notes-history-item {
        display: grid;
        width: 100%;
        gap: 2px;
        padding: 8px 9px;
        border: 1px solid transparent;
        border-radius: 6px;
        background: transparent;
        color: var(--color-text-secondary);
        font: inherit;
        font-size: 12px;
        text-align: left;
        cursor: pointer;
    }
    .notes-history-item:hover,
    .notes-history-item.is-active {
        border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
        color: var(--color-text);
    }
    .notes-history-item.is-active { color: var(--color-accent); }
    .notes-history-item small {
        overflow: hidden;
        color: var(--color-muted);
        font-size: 10.5px;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .notes-history-detail {
        display: grid;
        min-width: 0;
        min-height: 0;
        grid-template-rows: auto minmax(0, 1fr);
    }
    .notes-history-detail-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 10px 12px;
        border-bottom: 1px solid var(--color-border);
    }
    .notes-history-detail-head > div { min-width: 0; display: grid; gap: 2px; }
    .notes-history-detail-head strong { color: var(--color-text); font-size: 12px; }
    .notes-history-detail-head span { color: var(--color-muted); font-size: 10.5px; }
    .notes-history-restore {
        flex: none;
        display: inline-flex;
        align-items: center;
        gap: 5px;
        min-height: 29px;
        padding: 0 9px;
        border: 1px solid color-mix(in srgb, var(--color-accent) 48%, var(--color-border));
        border-radius: 6px;
        background: transparent;
        color: var(--color-accent);
        font: inherit;
        font-size: 11px;
        font-weight: 700;
        cursor: pointer;
    }
    .notes-history-restore:disabled { color: var(--color-muted); cursor: default; }
    .notes-history-restore :global(svg) { width: 13px; height: 13px; }
    .notes-history-diff {
        min-width: 0;
        margin: 0;
        padding: 12px;
        overflow: auto;
        color: var(--color-text-secondary);
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11px;
        line-height: 1.55;
        white-space: pre-wrap;
        word-break: break-word;
    }
    .notes-history-diff span {
        display: block;
        margin: 0 -12px;
        padding: 0 12px;
    }
    .notes-history-diff .diff-add {
        background: color-mix(in srgb, var(--color-success, #3a9b61) 16%, transparent);
        color: color-mix(in srgb, var(--color-success, #3a9b61) 78%, var(--color-text));
    }
    .notes-history-diff .diff-remove {
        background: color-mix(in srgb, var(--color-error) 14%, transparent);
        color: color-mix(in srgb, var(--color-error) 76%, var(--color-text));
    }
    .notes-history-diff .diff-hunk {
        background: color-mix(in srgb, var(--color-accent) 10%, transparent);
        color: var(--color-accent);
    }
    .notes-history-diff .diff-file { color: var(--color-muted); }
    .notes-history-empty {
        margin: 8px 0;
        color: var(--color-muted);
        font-size: 12px;
        line-height: 1.45;
    }

    .notes-capture {
        width: min(560px, calc(100vw - 32px));
        max-height: min(720px, calc(100vh - 32px));
        padding: 0;
        overflow: auto;
        border: 1px solid var(--color-border);
        border-radius: 12px;
        background: var(--color-bg);
        color: var(--color-text);
        box-shadow: var(--shadow-lg, 0 24px 70px rgb(0 0 0 / 0.34));
    }
    .notes-capture::backdrop {
        background: color-mix(in srgb, var(--color-text) 34%, transparent);
    }
    .notes-capture-form {
        display: grid;
        gap: 14px;
        padding: 18px;
    }
    .notes-capture-head {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 16px;
    }
    .notes-capture-kicker {
        margin: 0 0 3px;
        color: var(--color-accent);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.07em;
        text-transform: uppercase;
    }
    .notes-capture-head h2 {
        margin: 0;
        color: var(--color-text);
        font-size: 17px;
        letter-spacing: -0.02em;
    }
    .notes-capture-field {
        display: grid;
        gap: 6px;
        color: var(--color-text-secondary);
        font-size: 11px;
        font-weight: 650;
    }
    .notes-capture-field em {
        color: var(--color-muted);
        font-style: normal;
        font-weight: 500;
    }
    .notes-capture-field input,
    .notes-capture-field textarea {
        width: 100%;
        box-sizing: border-box;
        border: 1px solid var(--color-border);
        border-radius: 7px;
        background: transparent;
        color: var(--color-text);
        font: inherit;
        font-size: 13px;
    }
    .notes-capture-field input { height: 34px; padding: 0 9px; }
    .notes-capture-field textarea {
        min-height: 190px;
        padding: 9px;
        line-height: 1.5;
        resize: vertical;
    }
    .notes-capture-field input:focus,
    .notes-capture-field textarea:focus {
        outline: 2px solid color-mix(in srgb, var(--color-accent) 50%, transparent);
        outline-offset: 1px;
    }
    .notes-capture-tools {
        display: flex;
        gap: 14px;
        margin-top: -4px;
    }
    .notes-capture-link,
    .notes-capture-cancel {
        padding: 3px 0;
        border: none;
        background: transparent;
        color: var(--color-text-secondary);
        font: inherit;
        font-size: 11.5px;
        font-weight: 650;
        cursor: pointer;
    }
    .notes-capture-link:hover,
    .notes-capture-cancel:hover { color: var(--color-accent); }
    .notes-capture-actions {
        display: flex;
        align-items: center;
        justify-content: flex-end;
        gap: 14px;
        padding-top: 2px;
    }
    .notes-capture-save {
        min-height: 32px;
        padding: 0 12px;
        border: 1px solid color-mix(in srgb, var(--color-accent) 48%, var(--color-border));
        border-radius: 7px;
        background: transparent;
        color: var(--color-accent);
        font: inherit;
        font-size: 12px;
        font-weight: 700;
        cursor: pointer;
    }
    .notes-capture-save:disabled { color: var(--color-muted); cursor: default; }

    @media (max-width: 1050px) {
        .notes-split { flex-basis: 300px; min-width: 240px; }
        .notes-context { flex-basis: 250px; width: 250px; }
    }
    @media (max-width: 650px) {
        .notes-history-layout { grid-template-columns: 1fr; max-height: min(620px, calc(100vh - 180px)); }
        .notes-history-list { max-height: 145px; border-right: none; border-bottom: 1px solid var(--color-border); }
        .notes-history-detail-head { align-items: flex-start; }
        .notes-history-restore { white-space: nowrap; }
    }

    .notes-empty {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 12px;
        padding: 40px;
        text-align: center;
    }
    .notes-empty-ico {
        width: 52px;
        height: 52px;
        display: grid;
        place-items: center;
        border-radius: 14px;
        background: color-mix(in srgb, var(--color-accent) 12%, transparent);
        color: var(--color-accent);
    }
    .notes-empty-ico :global(svg) {
        width: 24px;
        height: 24px;
    }
    .notes-empty-title {
        margin: 0;
        font-size: 18px;
        font-weight: 700;
        color: var(--color-text);
    }
    .notes-empty-desc {
        margin: 0;
        max-width: 46ch;
        font-size: 13px;
        line-height: 1.55;
        color: var(--color-text-secondary);
    }
    .notes-empty-desc code {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        padding: 1px 5px;
        border-radius: 4px;
        background: var(--color-panel-2);
        color: var(--color-accent);
    }
    .notes-empty-new {
        margin-top: 6px;
        height: 36px;
        padding: 0 16px;
    }
</style>
