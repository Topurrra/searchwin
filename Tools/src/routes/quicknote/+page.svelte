<script lang="ts">
    /*
      Quick Note — a sticky note that uses the SAME editor as the Notes page
      (TipTap, via NotesEditor), so its formatting commands, active-state
      highlighting, and Markdown round-trip are identical. On change we keep the
      serialized Markdown and autosave a `.ki` file, so quick notes open + render
      in the full Notes app and are findable in search. The window is created on
      demand and DESTROYED on close (see lib.rs), freeing its WebView2 memory —
      so to PARK a note rather than dismiss it, minimize (the note keeps its
      window and its identity); close is the deliberate throw-away exit.

      The `.ki` frontmatter shape mirrors `$lib/stores/notes` serializeNote (kept
      in sync by hand to avoid pulling that store's notes-list machinery in).

      Per-note theme: each window picks its own look (refined dark / warm paper /
      accent header) via the header swatches — applied by overriding the design
      tokens on `.qn`, which cascade into the embedded editor too.
    */
    import { onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
    import NotesEditor from '$lib/tools/Notes/NotesEditor.svelte';
    import { INBOX_FOLDER, parseNote, type NoteFile, type NoteWriteResult } from '$lib/stores/notes';

    type Theme = 'dark' | 'paper' | 'header' | 'crimson';

    let body = $state(''); // serialized Markdown — source of truth for saving
    let saveState = $state<'idle' | 'saving' | 'saved' | 'conflict'>('idle');
    let theme = $state<Theme>('dark');
    let editorKey = $state(0);
    let conflict = $state<NoteWriteResult['conflict']>(null);

    let notePath: string | null = null;
    let createdAt: number | null = null;
    let revision: string | null = null;
    let dirty = false;
    let saveTimer: ReturnType<typeof setTimeout> | null = null;
    let saving: Promise<boolean> | null = null;
    /** Last empty/non-empty state pushed to the backend registry, so we only
     *  invoke on transitions (not every keystroke). */
    let lastReportedEmpty: boolean | null = null;

    const win = getCurrentWebviewWindow();

    function onBodyChange(markdown: string): void {
        body = markdown;
        reportEmpty();
        scheduleSave();
    }

    /** Tell the backend whether this note is empty, only on transitions (not
     *  every keystroke). Drives the sticky-note hotkey dedup: summon_quick_note
     *  raises an existing empty note instead of opening a duplicate blank. */
    function reportEmpty(): void {
        const empty = !body.trim();
        if (empty === lastReportedEmpty) return;
        lastReportedEmpty = empty;
        void invoke('set_quick_note_empty', { label: win.label, empty }).catch(() => {});
    }

    // ── Title + Markdown file building ───────────────────────────────────
    /** First non-empty line with markdown markers stripped — the note title. */
    function deriveTitle(text: string): string {
        for (const raw of text.split('\n')) {
            const line = raw
                .replace(/^#+\s*/, '')
                .replace(/^[-*]\s+/, '')
                .replace(/[*_`~]/g, '')
                .trim();
            if (line) return line.slice(0, 60);
        }
        return 'Quick note';
    }

    function yamlString(s: string): string {
        if (/[:#[\]{}"'\n]/.test(s)) {
            return '"' + s.replace(/\\/g, '\\\\').replace(/"/g, '\\"') + '"';
        }
        return s;
    }

    function buildKi(text: string, updatedAt: number): string {
        const created = createdAt ?? updatedAt;
        createdAt = created;
        return (
            `---\n` +
            `title: ${yamlString(deriveTitle(text))}\n` +
            `created: ${created}\n` +
            `updated: ${updatedAt}\n` +
            `pinned: false\n` +
            `tags: []\n` +
            `---\n\n` +
            text
        );
    }

    let editGeneration = 0;

    async function saveCurrentDraft(): Promise<boolean> {
        if (conflict) return false;
        if (!dirty) return true;
        // A never-typed quick note should not create a junk file. Once it
        // exists, though, an empty body is a real edit and must be persisted.
        if (!body.trim() && !notePath) {
            dirty = false;
            saveState = 'idle';
            return true;
        }

        const savingGeneration = editGeneration;
        const content = buildKi(body, Date.now());
        saveState = 'saving';
        try {
            if (!notePath) {
                const summary = await invoke<{ path: string }>('create_note', {
                    fileBase: deriveTitle(body),
                    content,
                    targetFolder: INBOX_FOLDER,
                });
                notePath = summary.path;
                const created = await invoke<NoteFile>('read_note', { path: notePath });
                if (created.content !== content) {
                    revision = created.revision;
                    conflict = 'changed';
                    dirty = true;
                    saveState = 'conflict';
                    return false;
                }
                revision = created.revision;
            } else {
                const result = await invoke<NoteWriteResult>('write_note', {
                    path: notePath,
                    content,
                    expectedRevision: revision,
                });
                if (!result.saved) {
                    revision = result.revision;
                    conflict = result.conflict ?? 'changed';
                    dirty = true;
                    saveState = 'conflict';
                    return false;
                }
                revision = result.revision;
            }
        } catch {
            dirty = true;
            saveState = 'idle';
            return false;
        }

        if (editGeneration !== savingGeneration) {
            dirty = true;
            return saveCurrentDraft();
        }

        dirty = false;
        saveState = 'saved';
        if (notePath) {
            void invoke('update_notes_index', {
                upserts: [notePath],
                deletes: [],
            }).catch(() => {});
        }
        return true;
    }

    async function flushSave(): Promise<boolean> {
        if (saveTimer) {
            clearTimeout(saveTimer);
            saveTimer = null;
        }
        if (saving) return saving;

        const pending = saveCurrentDraft();
        saving = pending;
        try {
            return await pending;
        } finally {
            if (saving === pending) saving = null;
        }
    }

    function scheduleSave(): void {
        dirty = true;
        editGeneration += 1;
        if (conflict) return;
        saveState = 'saving';
        if (saveTimer) clearTimeout(saveTimer);
        saveTimer = setTimeout(() => {
            saveTimer = null;
            void flushSave();
        }, 600);
    }

    async function reloadExternalNote(): Promise<void> {
        if (!notePath || !conflict) return;
        try {
            const loaded = await invoke<NoteFile>('read_note', { path: notePath });
            const parsed = parseNote(loaded.content);
            body = parsed.body;
            if (parsed.meta.created > 0) createdAt = parsed.meta.created;
            revision = loaded.revision;
            conflict = null;
            dirty = false;
            editGeneration = 0;
            saveState = 'saved';
            editorKey += 1;
            reportEmpty();
        } catch {
            // The missing-file conflict remains actionable through Keep mine.
            saveState = 'conflict';
        }
    }

    async function keepMine(): Promise<void> {
        if (!conflict) return;
        conflict = null;
        dirty = true;
        editGeneration += 1;
        await flushSave();
    }

    async function closeWindow(): Promise<void> {
        if (saveTimer) {
            clearTimeout(saveTimer);
            saveTimer = null;
        }
        const saved = await flushSave();
        if (!saved || conflict || dirty) return;
        await win.close();
    }

    /* ── Minimize / maximize ─────────────────────────────────────────────
       Close used to be the ONLY way out of a quick note, and it destroys
       the window — so parking a note meant losing it as a window. Native
       OS minimize works here because the window is built with
       skip_taskbar(false) (lib.rs), so it lands in the taskbar and comes
       back from there; lib.rs already calls unminimize() on the summon
       path, which was a defensive no-op until now.

       Safe against the transparent/undecorated window rules in §2.6: these
       are OS window-state calls, not DWM effects, and add no entrance
       animation. */
    let maximized = $state(false);

    async function minimizeWindow(): Promise<void> {
        // Flush first: a minimized note may sit for hours, and the 600ms
        // debounce shouldn't be what stands between it and disk.
        await flushSave();
        await win.minimize();
    }

    async function toggleMaximize(): Promise<void> {
        await win.toggleMaximize();
        maximized = await win.isMaximized();
    }

    function onKeydown(e: KeyboardEvent): void {
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
            e.preventDefault();
            if (saveTimer) {
                clearTimeout(saveTimer);
                saveTimer = null;
            }
            void flushSave();
        } else if (e.key === 'Escape') {
            e.preventDefault();
            void closeWindow();
        }
    }

    onMount(() => {
        // Frameless + transparent window → paint our own rounded card and clip
        // the viewport so no square backdrop leaks past the corners.
        for (const el of [document.documentElement, document.body]) {
            el.style.background = 'transparent';
            el.style.margin = '0';
            el.style.padding = '0';
            el.style.overflow = 'hidden';
            el.style.borderRadius = '14px';
            el.style.clipPath = 'inset(0 round 14px)';
        }
        const onBlur = () => void flushSave();
        const onBeforeUnload = () => void flushSave();
        window.addEventListener('blur', onBlur);
        window.addEventListener('beforeunload', onBeforeUnload);
        // Seed the emptiness registry (a fresh note starts empty) so the sticky
        // note hotkey can find + raise this note instead of spawning a dup.
        reportEmpty();

        // The OS can maximize/restore behind our back (Win+Up/Win+Down, drag to
        // the top edge, double-click the drag region), so the button's icon
        // can't be driven by our own click alone or it goes out of sync.
        void win.isMaximized().then((m) => (maximized = m));
        const unlistenResized = win.onResized(() => {
            void win.isMaximized().then((m) => (maximized = m));
        });

        return () => {
            window.removeEventListener('blur', onBlur);
            window.removeEventListener('beforeunload', onBeforeUnload);
            void unlistenResized.then((off) => off());
        };
    });
</script>

<svelte:window onkeydown={onKeydown} />

<div
    class="qn"
    class:theme-paper={theme === 'paper'}
    class:theme-header={theme === 'header'}
    class:theme-crimson={theme === 'crimson'}
>
    <div class="qn-accent" aria-hidden="true"></div>
    <header class="qn-bar" data-tauri-drag-region>
        <span class="qn-title" data-tauri-drag-region>Quick note</span>
        <span class="qn-status" aria-live="polite">
            {#if saveState === 'saving'}Saving…{:else if saveState === 'saved'}Saved{/if}
        </span>
        <div class="qn-swatches" role="group" aria-label="Note theme">
            <button
                class="qn-swatch sw-dark"
                class:is-active={theme === 'dark'}
                type="button"
                title="Refined dark"
                aria-label="Refined dark theme"
                onclick={() => (theme = 'dark')}
            ></button>
            <button
                class="qn-swatch sw-paper"
                class:is-active={theme === 'paper'}
                type="button"
                title="Warm paper"
                aria-label="Warm paper theme"
                onclick={() => (theme = 'paper')}
            ></button>
            <button
                class="qn-swatch sw-header"
                class:is-active={theme === 'header'}
                type="button"
                title="Emerald header"
                aria-label="Emerald header theme"
                onclick={() => (theme = 'header')}
            ></button>
            <button
                class="qn-swatch sw-crimson"
                class:is-active={theme === 'crimson'}
                type="button"
                title="Crimson header"
                aria-label="Crimson header theme"
                onclick={() => (theme = 'crimson')}
            ></button>
        </div>
        <!-- Minimize/maximize sit left of close, matching the OS caption
             order users already expect from a titlebar. -->
        <button
            class="qn-win-btn"
            type="button"
            onclick={() => void minimizeWindow()}
            aria-label="Minimize note"
            title="Minimize to taskbar"
        >
            &#x2500;
        </button>
        <button
            class="qn-win-btn"
            type="button"
            onclick={() => void toggleMaximize()}
            aria-label={maximized ? 'Restore note' : 'Maximize note'}
            title={maximized ? 'Restore' : 'Maximize'}
        >
            {maximized ? '❐' : '□'}
        </button>
        <button
            class="qn-close"
            type="button"
            onclick={() => void closeWindow()}
            aria-label="Close note"
            title="Save & close (Esc)"
        >
            ✕
        </button>
    </header>
    {#if conflict}
        <div class="qn-conflict" role="alert">
            <span>{conflict === 'missing' ? 'This note was deleted elsewhere.' : 'This note changed elsewhere.'}</span>
            <div class="qn-conflict-actions">
                {#if conflict !== 'missing'}
                    <button type="button" onclick={() => void reloadExternalNote()}>Reload</button>
                {/if}
                <button type="button" class="qn-conflict-keep" onclick={() => void keepMine()}>Keep mine</button>
            </div>
        </div>
    {/if}
    <div class="qn-editor">
        {#key editorKey}
            <NotesEditor value={body} onChange={onBodyChange} table={false} autofocus />
        {/key}
    </div>
</div>

<style>
    .qn {
        display: flex;
        flex-direction: column;
        height: 100vh;
        background: var(--color-panel, #1b1b1b);
        color: var(--color-text, #ececec);
        border: 1px solid var(--color-border, #2c2c2c);
        border-radius: 14px;
        overflow: hidden;
        font-family:
            'Inter',
            system-ui,
            -apple-system,
            sans-serif;
    }
    .qn-accent {
        flex: none;
        height: 3px;
        background: var(--color-accent, #10b981);
    }
    .qn-bar {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 7px 8px 7px 12px;
        background: var(--color-panel-2, #232323);
        border-bottom: 1px solid var(--color-border, #2c2c2c);
        cursor: default;
        user-select: none;
    }
    .qn-title {
        font-size: 12px;
        font-weight: 600;
        color: var(--color-text-secondary, #b5b5b5);
        flex: 1;
    }
    .qn-status {
        font-size: 11px;
        color: var(--color-accent, #10b981);
        min-width: 40px;
        text-align: right;
    }
    .qn-conflict {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 7px 12px;
        background: color-mix(in srgb, var(--color-danger, #ef4444) 10%, var(--color-panel));
        border-bottom: 1px solid color-mix(in srgb, var(--color-danger, #ef4444) 24%, var(--color-border));
        color: var(--color-text, #ececec);
        font-size: 11px;
    }
    .qn-conflict > span {
        flex: 1;
    }
    .qn-conflict-actions {
        display: flex;
        gap: 5px;
    }
    .qn-conflict button {
        border: 1px solid var(--color-border, #2c2c2c);
        border-radius: 5px;
        padding: 3px 7px;
        background: transparent;
        color: inherit;
        font: inherit;
        cursor: pointer;
    }
    .qn-conflict button:hover {
        background: color-mix(in srgb, var(--color-text, #ececec) 10%, transparent);
    }
    .qn-conflict button:focus-visible {
        outline: 2px solid var(--color-accent, #10b981);
        outline-offset: 1px;
    }
    .qn-conflict .qn-conflict-keep {
        border-color: var(--color-accent, #10b981);
        color: var(--color-accent, #10b981);
    }
    .qn-swatches {
        display: flex;
        align-items: center;
        gap: 5px;
    }
    .qn-swatch {
        width: 13px;
        height: 13px;
        border-radius: 50%;
        border: 1px solid rgba(0, 0, 0, 0.25);
        padding: 0;
        cursor: pointer;
        opacity: 0.5;
        transition:
            opacity 120ms ease,
            box-shadow 120ms ease;
    }
    .qn-swatch:hover {
        opacity: 0.85;
    }
    .qn-swatch.is-active {
        opacity: 1;
        box-shadow: 0 0 0 2px rgba(150, 150, 150, 0.55);
    }
    .sw-dark {
        background: #1c1c1e;
    }
    .sw-paper {
        background: #faf6ea;
    }
    /* Swatches preview each theme's accent literally, so they're hardcoded
       rather than tokenised — a token would resolve to the ACTIVE theme's
       accent and every swatch would look identical. */
    .sw-header {
        background: #10b981;
    }
    .sw-crimson {
        background: #b5352c;
    }
    .qn-close {
        flex: none;
        width: 22px;
        height: 22px;
        display: grid;
        place-items: center;
        border: none;
        border-radius: 6px;
        background: transparent;
        color: var(--color-text-secondary, #b5b5b5);
        font-size: 12px;
        cursor: pointer;
        transition:
            background 120ms ease,
            color 120ms ease;
    }
    .qn-close:hover {
        background: color-mix(in srgb, var(--color-danger, #ef4444) 20%, transparent);
        color: var(--color-danger, #ef4444);
    }
    /* Same metrics as .qn-close so the three caption buttons form an even
       row; only the hover treatment differs — neutral here, danger there,
       because only one of them destroys the note. */
    .qn-win-btn {
        flex: none;
        width: 22px;
        height: 22px;
        display: grid;
        place-items: center;
        border: none;
        border-radius: 6px;
        background: transparent;
        color: var(--color-text-secondary, #b5b5b5);
        font-size: 11px;
        line-height: 1;
        cursor: pointer;
        transition:
            background 120ms ease,
            color 120ms ease;
    }
    .qn-win-btn:hover {
        background: color-mix(in srgb, var(--color-text, #ececec) 12%, transparent);
        color: var(--color-text, #ececec);
    }
    .qn-editor {
        flex: 1;
        min-height: 0;
        display: flex;
        flex-direction: column;
    }
    .qn-editor :global(.ne) {
        flex: 1;
        min-height: 0;
    }
    /* Tighten the shared editor's generous padding for the small sticky window. */
    .qn-editor :global(.tiptap.ProseMirror) {
        padding: 12px 14px 28px;
        font-size: 14px;
    }

    /* Per-note themes: override the design tokens on `.qn`; custom properties
       cascade into the embedded NotesEditor, so the whole note re-themes. */
    .qn.theme-paper {
        --color-panel: #faf6ea;
        --color-panel-2: #f1e9d4;
        --color-text: #463d27;
        --color-text-secondary: #8a7a54;
        --color-border: #e7dcc0;
        --color-accent: #b0862a;
        --color-muted: #b3a988;
    }
    /* Accent-header themes: identical layout, different accent. Emerald is the
       original; crimson matches the 2026-07 rebrand. Both stay — the sticky
       note's colour is the user's choice, not a brand surface. Add another by
       cloning the token block and adding its selector to the shared rules
       below; don't fork the layout. */
    .qn.theme-header,
    .qn.theme-crimson {
        --color-panel: #ffffff;
        --color-panel-2: #f3f5f7;
        --color-text: #2b3037;
        --color-text-secondary: #5b6470;
        --color-border: #e2e6ea;
        --color-muted: #97a0ab;
    }
    .qn.theme-header {
        --color-accent: #10b981;
    }
    .qn.theme-crimson {
        --color-accent: #b5352c;
    }
    .qn.theme-header .qn-accent,
    .qn.theme-crimson .qn-accent {
        display: none;
    }
    .qn.theme-header .qn-bar,
    .qn.theme-crimson .qn-bar {
        background: var(--color-accent);
        border-bottom: none;
    }
    .qn.theme-header .qn-title,
    .qn.theme-crimson .qn-title {
        color: #ffffff;
    }
    .qn.theme-header .qn-status,
    .qn.theme-crimson .qn-status {
        color: rgba(255, 255, 255, 0.85);
    }
    /* .qn-win-btn (minimize/maximize) is included here deliberately: without
       it those two sit near-invisible in dark grey on the coloured bar. */
    .qn.theme-header .qn-close,
    .qn.theme-crimson .qn-close,
    .qn.theme-header .qn-win-btn,
    .qn.theme-crimson .qn-win-btn {
        color: rgba(255, 255, 255, 0.85);
    }
    .qn.theme-header .qn-close:hover,
    .qn.theme-crimson .qn-close:hover,
    .qn.theme-header .qn-win-btn:hover,
    .qn.theme-crimson .qn-win-btn:hover {
        background: rgba(255, 255, 255, 0.2);
        color: #ffffff;
    }
</style>
