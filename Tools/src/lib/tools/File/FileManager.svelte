<script lang="ts">
    /*
      FileManager — a dual-pane (Total Commander style) file manager.

      Two panes side by side with a draggable vertical divider. Each
      pane has a path bar + breadcrumb, a drive switcher, an Up button,
      and a sortable multi-select file list. An action bar between the
      panes copies/moves/deletes/renames the ACTIVE pane's selection
      into the OTHER pane (or in place).

      All persistent pane state lives in $lib/stores/fileManager so the
      Category Workspace's {#key} unmount doesn't wipe where the user
      was. Only ephemeral UI (splitter fraction, drag state, dialogs)
      is component-local $state.
    */
    import { onMount, onDestroy, tick } from 'svelte';
    import { get } from 'svelte/store';
    import { invoke } from '@tauri-apps/api/core';
    import { listen, type UnlistenFn } from '@tauri-apps/api/event';
    import { getCurrentWebview } from '@tauri-apps/api/webview';
    import { open as openDialog } from '@tauri-apps/plugin-dialog';
    import {
        Columns2,
        HardDrive,
        ArrowUp,
        ArrowRightLeft,
        Copy,
        Trash2,
        FilePenLine,
        FolderPlus,
        FolderSync,
        ChevronRight,
        RotateCw,
        X,
        Eye,
        EyeOff,
        Folder,
        Sigma,
        Loader2,
    } from '@lucide/svelte';

    import ToolPage from '$lib/ui/ToolPage.svelte';
    import Button from '$lib/ui/Button.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import ErrorState from '$lib/ui/ErrorState.svelte';
    import { toast } from '$lib/stores/toasts';
    import { confirm } from '$lib/stores/confirmDialog';

    import {
        leftPane,
        rightPane,
        activePane,
        driveList,
        fmOpProgress,
        fmBusy,
        fmRunningOp,
        initFileManager,
        loadDir,
        loadDrives,
        navigateTo,
        goUp,
        setSort,
        setShowHidden,
        setFocusIndex,
        selectSingle,
        toggleSelect,
        selectRange,
        clearSelection,
        selectedEntries,
        otherPane,
        iconForEntry,
        formatBytes,
        formatModified,
        folderSizes,
        showFolderSizes,
        setShowFolderSizes,
        type PaneId,
        type SortBy,
        type FmEntry,
        type FmOpResult,
        type FmDeleteResult,
        type FmProgress,
    } from '$lib/stores/fileManager';

    // ─── Ephemeral, component-local UI state ────────────────────────────

    /** Splitter fraction (left pane width as a 0.2–0.8 fraction). */
    let splitFraction = $state(0.5);
    let dividerDragging = $state(false);
    let containerEl = $state<HTMLDivElement | null>(null);

    /** Internal (pane→pane) drag state. */
    let internalDrag = $state<{ from: PaneId; paths: string[] } | null>(null);
    /** Which pane is currently a drop target (for highlight). */
    let dropTarget = $state<PaneId | null>(null);
    /** OS file drop hover (Tauri drag-drop), keyed to pane under pointer. */
    let osDropTarget = $state<PaneId | null>(null);

    /** Editable path-bar buffers (separate from the committed store path
     *  so typing doesn't trigger a load every keystroke). */
    let pathDraftLeft = $state('');
    let pathDraftRight = $state('');

    /** Path-bar autocomplete (per pane, ephemeral): matching child-folder paths,
     *  the highlighted row, and whether the dropdown is open. */
    let pathSug = $state<Record<PaneId, string[]>>({ left: [], right: [] });
    let pathSugIndex = $state<Record<PaneId, number>>({ left: -1, right: -1 });
    let pathSugOpen = $state<Record<PaneId, boolean>>({ left: false, right: false });
    const sugTimers: Record<PaneId, ReturnType<typeof setTimeout> | null> = {
        left: null,
        right: null,
    };

    /** Inline rename dialog. */
    let renameOpen = $state(false);
    let renameValue = $state('');
    let renameTargetPath = $state('');
    let renameTargetPane = $state<PaneId>('left');

    /** New-folder dialog. */
    let newFolderOpen = $state(false);
    let newFolderName = $state('');
    let newFolderPane = $state<PaneId>('left');

    /** Backup dialog. */
    let backupOpen = $state(false);
    let backupSource = $state('');
    let backupDest = $state('');
    let backupMirror = $state(false);
    let backupPane = $state<PaneId>('left');

    let busy = $derived($fmBusy);

    // ─── Derived store snapshots ────────────────────────────────────────

    const panes: Record<PaneId, typeof leftPane> = { left: leftPane, right: rightPane };

    let left = $derived($leftPane);
    let right = $derived($rightPane);
    let drives = $derived($driveList);
    let active = $derived($activePane);
    let progress = $derived($fmOpProgress);
    let runningOp = $derived($fmRunningOp);

    function paneData(pane: PaneId) {
        return pane === 'left' ? left : right;
    }

    // Keep the editable path drafts in sync when the committed path changes
    // (drive switch, Up, double-click) but NOT while the user is editing.
    $effect(() => {
        pathDraftLeft = left.path;
    });
    $effect(() => {
        pathDraftRight = right.path;
    });

    // ─── Lifecycle ──────────────────────────────────────────────────────

    let unlistenProgress: UnlistenFn | null = null;
    let unlistenOsDrop: UnlistenFn | null = null;
    // The Category Workspace wraps this tool in {#key}, so a nav away/back can
    // destroy the component mid-await. Guard each async listener registration so
    // one that resolves after destroy is torn down immediately instead of
    // leaking (and stacking another live OS-drop listener on every remount).
    let destroyed = false;

    onMount(async () => {
        await initFileManager();

        const up = await listen<FmProgress>('fm-progress', (event) => {
            fmOpProgress.set(event.payload);
        });
        if (destroyed) up();
        else unlistenProgress = up;

        // OS file drops (drag from Explorer onto a pane → copy here).
        try {
            const webview = getCurrentWebview();
            const ud = await webview.onDragDropEvent((event) => {
                if (event.payload.type === 'enter' || event.payload.type === 'over') {
                    osDropTarget = paneFromPosition(event.payload.position);
                } else if (event.payload.type === 'leave') {
                    osDropTarget = null;
                } else if (event.payload.type === 'drop') {
                    const target = paneFromPosition(event.payload.position) ?? osDropTarget;
                    osDropTarget = null;
                    const paths = event.payload.paths ?? [];
                    if (target && paths.length) void copyOsPaths(paths, target);
                }
            });
            if (destroyed) ud();
            else unlistenOsDrop = ud;
        } catch {
            // OS drop is best-effort; pane→pane drag still works.
        }
    });

    onDestroy(() => {
        destroyed = true;
        if (unlistenProgress) try { unlistenProgress(); } catch { /* ignore */ }
        if (unlistenOsDrop) try { unlistenOsDrop(); } catch { /* ignore */ }
    });

    /** Which pane an OS drop landed on, from the drag event's OWN position
     *  (device px → CSS px). DOM pointermove doesn't fire during a native OS
     *  drag, so the event position is the only reliable source. Bounds-checked
     *  on both axes so a drop on the action bar / outside the panes is ignored. */
    function paneFromPosition(pos: { x: number; y: number } | undefined): PaneId | null {
        if (!containerEl || !pos) return null;
        const rect = containerEl.getBoundingClientRect();
        const dpr = window.devicePixelRatio || 1;
        const x = pos.x / dpr;
        const y = pos.y / dpr;
        if (x < rect.left || x > rect.right || y < rect.top || y > rect.bottom) return null;
        const rel = (x - rect.left) / rect.width;
        return rel < splitFraction ? 'left' : 'right';
    }

    // ─── Splitter drag ──────────────────────────────────────────────────

    function onDividerDown(e: PointerEvent) {
        dividerDragging = true;
        (e.target as HTMLElement).setPointerCapture(e.pointerId);
        e.preventDefault();
    }
    function onDividerMove(e: PointerEvent) {
        if (!dividerDragging || !containerEl) return;
        const rect = containerEl.getBoundingClientRect();
        const frac = (e.clientX - rect.left) / rect.width;
        splitFraction = Math.min(0.8, Math.max(0.2, frac));
    }
    function onDividerUp(e: PointerEvent) {
        if (!dividerDragging) return;
        dividerDragging = false;
        try { (e.target as HTMLElement).releasePointerCapture(e.pointerId); } catch { /* ignore */ }
    }

    // ─── Row interaction ────────────────────────────────────────────────

    function onRowClick(pane: PaneId, index: number, e: MouseEvent) {
        activePane.set(pane);
        if (e.shiftKey) {
            selectRange(pane, index);
        } else if (e.ctrlKey || e.metaKey) {
            toggleSelect(pane, index);
        } else {
            selectSingle(pane, index);
        }
    }

    function openEntry(pane: PaneId, entry: FmEntry) {
        if (entry.isDir) {
            void navigateTo(pane, entry.path);
        } else {
            // Open files with the OS default app via the shell-less
            // invoke path used elsewhere; fall back to a toast if absent.
            void invoke('open_search_result_path', { path: entry.path }).catch(() => {
                toast('Could not open this file', 'error');
            });
        }
    }

    function onRowDblClick(pane: PaneId, entry: FmEntry) {
        openEntry(pane, entry);
    }

    // ─── Keyboard navigation (RAF-coalesced, instant scroll) ────────────
    //
    // Mirrors the command palette's scheduler: a held arrow fires OS-rate
    // key events, so we coalesce all deltas within a frame and do a single
    // instant keep-in-view scroll instead of queuing smooth-scroll
    // animations (which is what made the palette lag).

    let pendingNavDelta = 0;
    let navRaf: number | null = null;
    let scrollEls: Record<PaneId, HTMLDivElement | null> = { left: null, right: null };

    function scheduleNavStep(pane: PaneId, delta: number) {
        pendingNavDelta += delta;
        if (navRaf !== null) return;
        navRaf = requestAnimationFrame(() => flushNav(pane));
    }

    function flushNav(pane: PaneId) {
        navRaf = null;
        const p = paneData(pane);
        const len = p.entries.length;
        if (len === 0) {
            pendingNavDelta = 0;
            return;
        }
        const current = p.focusIndex < 0 ? 0 : p.focusIndex;
        const next = Math.min(len - 1, Math.max(0, current + pendingNavDelta));
        pendingNavDelta = 0;
        selectSingle(pane, next);
        scrollFocusIntoView(pane, next);
    }

    function scrollFocusIntoView(pane: PaneId, index: number) {
        const scrollEl = scrollEls[pane];
        if (!scrollEl) return;
        const row = scrollEl.querySelector<HTMLElement>(`[data-row-index="${index}"]`);
        if (!row) return;
        const cRect = scrollEl.getBoundingClientRect();
        const rRect = row.getBoundingClientRect();
        if (rRect.top < cRect.top) {
            scrollEl.scrollTop -= cRect.top - rRect.top + 4;
        } else if (rRect.bottom > cRect.bottom) {
            scrollEl.scrollTop += rRect.bottom - cRect.bottom + 4;
        }
    }

    function onPaneKeydown(pane: PaneId, e: KeyboardEvent) {
        switch (e.key) {
            case 'ArrowDown':
                e.preventDefault();
                scheduleNavStep(pane, 1);
                break;
            case 'ArrowUp':
                e.preventDefault();
                scheduleNavStep(pane, -1);
                break;
            case 'Enter': {
                e.preventDefault();
                const p = paneData(pane);
                const entry = p.entries[p.focusIndex];
                if (entry) openEntry(pane, entry);
                break;
            }
            case 'Backspace':
                e.preventDefault();
                void goUp(pane);
                break;
            case 'Tab':
                e.preventDefault();
                switchActive();
                break;
            case 'Delete':
                e.preventDefault();
                void doDelete(pane);
                break;
            case 'a':
                if (e.ctrlKey || e.metaKey) {
                    e.preventDefault();
                    selectAll(pane);
                }
                break;
            default:
                break;
        }
    }

    function selectAll(pane: PaneId) {
        const store = panes[pane];
        store.update((p) => ({ ...p, selected: p.entries.map((en) => en.path) }));
    }

    function switchActive() {
        const next = otherPane(active);
        activePane.set(next);
        scrollEls[next]?.focus();
    }

    // ─── Drive switcher / path bar / breadcrumb ──────────────────────────

    function switchDrive(pane: PaneId, drive: string) {
        activePane.set(pane);
        void navigateTo(pane, drive);
    }

    function commitPath(pane: PaneId) {
        const draft = (pane === 'left' ? pathDraftLeft : pathDraftRight).trim();
        if (draft) void navigateTo(pane, draft);
    }

    // ─── Path-bar autocomplete ──────────────────────────────────────────

    /** Last path segment (folder name) of a full path, for the suggestion label. */
    function leafName(path: string): string {
        const norm = path.replace(/[/\\]+$/, '');
        const i = Math.max(norm.lastIndexOf('\\'), norm.lastIndexOf('/'));
        return i >= 0 ? norm.slice(i + 1) : norm;
    }

    function closeSuggest(pane: PaneId) {
        pathSugOpen[pane] = false;
        pathSugIndex[pane] = -1;
    }

    /** Debounced fetch of matching child folders for the typed path. */
    function scheduleSuggest(pane: PaneId, value: string) {
        if (sugTimers[pane]) clearTimeout(sugTimers[pane]!);
        sugTimers[pane] = setTimeout(async () => {
            try {
                const list = await invoke<string[]>('fm_path_suggestions', { input: value });
                pathSug[pane] = list;
                pathSugIndex[pane] = -1;
                pathSugOpen[pane] = list.length > 0;
            } catch {
                pathSug[pane] = [];
                pathSugOpen[pane] = false;
            }
        }, 120);
    }

    function onPathInput(pane: PaneId, value: string) {
        if (pane === 'left') pathDraftLeft = value;
        else pathDraftRight = value;
        scheduleSuggest(pane, value);
    }

    /** Navigate to a chosen suggestion (Enter on a highlighted row, or a click),
     *  then leave a trailing separator in the path bar so the next segment can be
     *  typed + suggested straight away. The path→draft `$effect` fires during
     *  navigation (resetting the draft to the slash-less store path), so we wait a
     *  tick for it to flush before appending the separator. */
    async function chooseSuggest(pane: PaneId, path: string) {
        closeSuggest(pane);
        pathSug[pane] = [];
        activePane.set(pane);
        await navigateTo(pane, path);
        await tick();
        const withSlash = /[/\\]$/.test(path) ? path : path + '\\';
        if (pane === 'left') pathDraftLeft = withSlash;
        else pathDraftRight = withSlash;
    }

    /** Tab-complete the input to the highlighted (or first) suggestion plus a
     *  trailing separator, then list THAT folder's children so you can drill in. */
    function completeSuggest(pane: PaneId) {
        const list = pathSug[pane];
        if (!list.length) return;
        const idx = pathSugIndex[pane] >= 0 ? pathSugIndex[pane] : 0;
        const chosen = list[idx];
        const completed = /[/\\]$/.test(chosen) ? chosen : chosen + '\\';
        if (pane === 'left') pathDraftLeft = completed;
        else pathDraftRight = completed;
        scheduleSuggest(pane, completed);
    }

    /** Key handling for the path input: arrows move the dropdown highlight,
     *  Tab completes, Enter executes (highlighted suggestion → navigate; else the
     *  typed path), Esc closes the dropdown. */
    function onPathKeydown(pane: PaneId, e: KeyboardEvent) {
        const open = pathSugOpen[pane];
        const list = pathSug[pane];
        if (open && list.length && e.key === 'ArrowDown') {
            e.preventDefault();
            pathSugIndex[pane] = (pathSugIndex[pane] + 1) % list.length;
        } else if (open && list.length && e.key === 'ArrowUp') {
            e.preventDefault();
            pathSugIndex[pane] = (pathSugIndex[pane] - 1 + list.length) % list.length;
        } else if (open && list.length && e.key === 'Tab') {
            e.preventDefault();
            completeSuggest(pane);
        } else if (e.key === 'Enter') {
            e.preventDefault();
            if (open && pathSugIndex[pane] >= 0) {
                chooseSuggest(pane, list[pathSugIndex[pane]]);
            } else {
                closeSuggest(pane);
                commitPath(pane);
            }
        } else if (e.key === 'Escape' && open) {
            e.preventDefault();
            closeSuggest(pane);
        }
    }

    /** Split a path into breadcrumb segments with their cumulative paths. */
    function breadcrumbs(path: string): { label: string; full: string }[] {
        const norm = path.replace(/[/\\]+$/, '');
        const parts = norm.split(/[/\\]/).filter(Boolean);
        const out: { label: string; full: string }[] = [];
        let acc = '';
        for (let i = 0; i < parts.length; i++) {
            const part = parts[i];
            if (i === 0 && /^[a-zA-Z]:$/.test(part)) {
                acc = part + '\\';
                out.push({ label: part, full: acc });
            } else {
                acc = acc.endsWith('\\') ? acc + part : acc + '\\' + part;
                out.push({ label: part, full: acc });
            }
        }
        return out;
    }

    // ─── Drag-and-drop between panes ────────────────────────────────────

    function onRowDragStart(pane: PaneId, entry: FmEntry, e: DragEvent) {
        // If the dragged row isn't in the selection, make it the selection.
        const p = paneData(pane);
        let paths = p.selected;
        if (!paths.includes(entry.path)) {
            selectSingle(pane, p.entries.indexOf(entry));
            paths = [entry.path];
        }
        internalDrag = { from: pane, paths: [...paths] };
        if (e.dataTransfer) {
            e.dataTransfer.effectAllowed = 'copyMove';
            e.dataTransfer.setData('text/plain', paths.join('\n'));
        }
    }

    function onPaneDragOver(pane: PaneId, e: DragEvent) {
        if (!internalDrag) return;
        if (internalDrag.from === pane) return; // no-op drop onto self
        e.preventDefault();
        dropTarget = pane;
        if (e.dataTransfer) {
            e.dataTransfer.dropEffect = e.ctrlKey || e.metaKey ? 'move' : 'copy';
        }
    }

    function onPaneDragLeave(pane: PaneId) {
        if (dropTarget === pane) dropTarget = null;
    }

    function onPaneDrop(pane: PaneId, e: DragEvent) {
        if (!internalDrag) return;
        e.preventDefault();
        const drag = internalDrag;
        internalDrag = null;
        dropTarget = null;
        if (drag.from === pane) return;
        const move = e.ctrlKey || e.metaKey;
        const destDir = paneData(pane).path;
        if (move) void runMove(drag.paths, destDir, drag.from, pane);
        else void runCopy(drag.paths, destDir, drag.from, pane);
    }

    function onRowDragEnd() {
        internalDrag = null;
        dropTarget = null;
    }

    // ─── Operations ─────────────────────────────────────────────────────

    async function copyOsPaths(paths: string[], pane: PaneId) {
        const destDir = paneData(pane).path;
        await runCopy(paths, destDir, null, pane);
    }

    async function runCopy(
        sources: string[],
        destDir: string,
        fromPane: PaneId | null,
        destPane: PaneId,
    ) {
        if (!sources.length) return toast('Nothing selected to copy', 'info');
        if (get(fmBusy)) return;
        const opId = `copy_${Date.now()}`;
        fmBusy.set(true);
        fmRunningOp.set(opId);
        fmOpProgress.set(null);
        try {
            const res = await invoke<FmOpResult>('fm_copy', {
                sources,
                destDir,
                onConflict: 'rename',
                operationId: opId,
            });
            reportOpResult('Copied', res);
        } catch (error) {
            reportOpError('Copy', error);
        } finally {
            fmBusy.set(false);
            fmRunningOp.set(null);
            fmOpProgress.set(null);
            await refreshPanes(fromPane, destPane);
        }
    }

    async function runMove(
        sources: string[],
        destDir: string,
        fromPane: PaneId | null,
        destPane: PaneId,
    ) {
        if (!sources.length) return toast('Nothing selected to move', 'info');
        if (get(fmBusy)) return;
        const opId = `move_${Date.now()}`;
        fmBusy.set(true);
        fmRunningOp.set(opId);
        fmOpProgress.set(null);
        try {
            const res = await invoke<FmOpResult>('fm_move', {
                sources,
                destDir,
                onConflict: 'rename',
                operationId: opId,
            });
            reportOpResult('Moved', res);
        } catch (error) {
            reportOpError('Move', error);
        } finally {
            fmBusy.set(false);
            fmRunningOp.set(null);
            fmOpProgress.set(null);
            await refreshPanes(fromPane, destPane);
        }
    }

    function reportOpResult(verb: string, res: FmOpResult) {
        if (res.failed > 0) {
            toast(`${verb} ${res.copied}, ${res.failed} failed`, 'error');
        } else if (res.skipped > 0) {
            toast(`${verb} ${res.copied}, ${res.skipped} skipped`, 'info');
        } else {
            toast(`${verb} ${res.copied} item${res.copied === 1 ? '' : 's'}`, 'success');
        }
    }

    function reportOpError(verb: string, error: unknown) {
        const message = error instanceof Error ? error.message : String(error);
        if (message === 'Cancelled') toast(`${verb} cancelled`, 'info');
        else toast(message, 'error');
    }

    async function refreshPanes(...paneIds: (PaneId | null)[]) {
        const unique = Array.from(new Set(paneIds.filter((p): p is PaneId => p !== null)));
        await Promise.all(unique.map((p) => loadDir(p)));
    }

    // Action-bar buttons act on the ACTIVE pane's selection into the OTHER.
    function actionCopy() {
        const from = active;
        const to = otherPane(active);
        const sources = selectedEntries(from).map((e) => e.path);
        void runCopy(sources, paneData(to).path, from, to);
    }

    function actionMove() {
        const from = active;
        const to = otherPane(active);
        const sources = selectedEntries(from).map((e) => e.path);
        void runMove(sources, paneData(to).path, from, to);
    }

    async function doDelete(pane: PaneId) {
        if (get(fmBusy)) return;
        const entries = selectedEntries(pane);
        if (!entries.length) return toast('Nothing selected to delete', 'info');
        const ok = await confirm(
            `Send ${entries.length} item${entries.length === 1 ? '' : 's'} to the Recycle Bin?`,
            { title: 'Delete to Recycle Bin', kind: 'warning', confirmLabel: 'Delete', danger: true },
        );
        if (!ok || get(fmBusy)) return;
        fmBusy.set(true);
        try {
            const res = await invoke<FmDeleteResult>('fm_delete_to_recycle', {
                paths: entries.map((e) => e.path),
            });
            if (res.errors.length) {
                toast(`Deleted ${res.deleted}, ${res.errors.length} failed`, 'error');
            } else {
                toast(`Deleted ${res.deleted} item${res.deleted === 1 ? '' : 's'}`, 'success');
            }
        } catch (error) {
            reportOpError('Delete', error);
        } finally {
            fmBusy.set(false);
            clearSelection(pane);
            await loadDir(pane);
        }
    }

    function actionDelete() {
        void doDelete(active);
    }

    // ─── Rename ─────────────────────────────────────────────────────────

    function actionRename() {
        const entries = selectedEntries(active);
        if (entries.length !== 1) return toast('Select exactly one item to rename', 'info');
        renameTargetPath = entries[0].path;
        renameValue = entries[0].name;
        renameTargetPane = active;
        renameOpen = true;
    }

    async function commitRename() {
        if (get(fmBusy)) return;
        const newName = renameValue.trim();
        if (!newName) return;
        fmBusy.set(true);
        try {
            await invoke<string>('fm_rename', { path: renameTargetPath, newName });
            toast('Renamed', 'success');
            renameOpen = false;
            await loadDir(renameTargetPane);
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            toast(message, 'error');
        } finally {
            fmBusy.set(false);
        }
    }

    // ─── New folder ─────────────────────────────────────────────────────

    function actionNewFolder() {
        newFolderPane = active;
        newFolderName = '';
        newFolderOpen = true;
    }

    async function commitNewFolder() {
        if (get(fmBusy)) return;
        const name = newFolderName.trim();
        if (!name) return;
        fmBusy.set(true);
        try {
            await invoke<string>('fm_make_dir', { parentDir: paneData(newFolderPane).path, name });
            toast('Folder created', 'success');
            newFolderOpen = false;
            await loadDir(newFolderPane);
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            toast(message, 'error');
        } finally {
            fmBusy.set(false);
        }
    }

    // ─── Backup (robocopy) ──────────────────────────────────────────────

    function actionBackup() {
        backupPane = active;
        backupSource = paneData(active).path;
        backupDest = paneData(otherPane(active)).path;
        backupMirror = false;
        backupOpen = true;
    }

    async function pickBackupDest() {
        const picked = await openDialog({ directory: true, multiple: false });
        if (typeof picked === 'string') backupDest = picked;
    }

    async function runBackup() {
        if (get(fmBusy)) return;
        if (!backupSource || !backupDest) return toast('Pick a source and destination', 'info');
        if (backupMirror) {
            const ok = await confirm(
                'Mirror makes the destination an EXACT replica of the source — any extra files in the destination will be DELETED. Continue?',
                { title: 'Mirror backup', kind: 'warning', confirmLabel: 'Mirror', danger: true },
            );
            if (!ok || get(fmBusy)) return;
        }
        const opId = `backup_${Date.now()}`;
        backupOpen = false;
        fmBusy.set(true);
        fmRunningOp.set(opId);
        fmOpProgress.set(null);
        try {
            const summary = await invoke<string>('fm_backup', {
                sourceDir: backupSource,
                destDir: backupDest,
                mirror: backupMirror,
                operationId: opId,
            });
            toast(summary || 'Backup complete', 'success');
        } catch (error) {
            reportOpError('Backup', error);
        } finally {
            fmBusy.set(false);
            fmRunningOp.set(null);
            fmOpProgress.set(null);
            // Refresh both panes — either could be the destination.
            await refreshPanes('left', 'right');
        }
    }

    // ─── Cancel ─────────────────────────────────────────────────────────

    async function cancelOp() {
        const opId = runningOp;
        if (!opId) return;
        try {
            await invoke('fm_cancel', { operationId: opId });
        } catch {
            // Best-effort; the op polls the flag.
        }
    }

    const sortOptions: { value: SortBy; label: string }[] = [
        { value: 'name', label: 'Name' },
        { value: 'size', label: 'Size' },
        { value: 'modified', label: 'Modified' },
    ];
</script>

<ToolPage
    icon={Columns2}
    iconTint="#64748b"
    title="File Manager"
    description="Work between two locations side by side. Select items, then copy, move, rename, or back up with full control."
    width="wide"
    fill={false}
>
    {#snippet children()}
        <div class="fm-shell">
                    <!-- Action bar -->
                    <div class="fm-actions">
                        <Button
                            variant="secondary"
                            size="sm"
                            icon={Copy}
                            disabled={busy}
                            onclick={actionCopy}
                            title={`Copy active selection to the ${otherPane(active)} pane`}
                        >
                            Copy {active === 'left' ? '→' : '←'}
                        </Button>
                        <Button
                            variant="secondary"
                            size="sm"
                            icon={ArrowRightLeft}
                            disabled={busy}
                            onclick={actionMove}
                            title={`Move active selection to the ${otherPane(active)} pane`}
                        >
                            Move {active === 'left' ? '→' : '←'}
                        </Button>
                        <div class="fm-actions-sep"></div>
                        <Button variant="secondary" size="sm" icon={FilePenLine} disabled={busy} onclick={actionRename}>
                            Rename
                        </Button>
                        <Button variant="secondary" size="sm" icon={FolderPlus} disabled={busy} onclick={actionNewFolder}>
                            New folder
                        </Button>
                        <Button variant="secondary" size="sm" icon={FolderSync} disabled={busy} onclick={actionBackup}>
                            Backup
                        </Button>
                        <div class="fm-actions-sep"></div>
                        <Button variant="danger" size="sm" icon={Trash2} disabled={busy} onclick={actionDelete}>
                            Delete
                        </Button>
                    </div>
            <!-- Panes + draggable divider -->
            <div
                class="fm-panes"
                bind:this={containerEl}
                style={`grid-template-columns: ${splitFraction}fr 6px ${1 - splitFraction}fr;`}
            >
                {#each ['left', 'right'] as const as paneId (paneId)}
                    {@const p = paneData(paneId)}
                    <section
                        class="fm-pane"
                        class:is-active={active === paneId}
                        class:is-drop={dropTarget === paneId || osDropTarget === paneId}
                        role="group"
                        aria-label={`${paneId} pane`}
                        ondragover={(e) => onPaneDragOver(paneId, e)}
                        ondragleave={() => onPaneDragLeave(paneId)}
                        ondrop={(e) => onPaneDrop(paneId, e)}
                    >
                        <!-- Path bar + breadcrumb -->
                        <div class="fm-pathbar">
                            <button
                                class="fm-up"
                                title="Up to parent"
                                disabled={!p.parent}
                                onclick={() => goUp(paneId)}
                            >
                                <ArrowUp class="fm-ico" />
                            </button>
                            <input
                                class="fm-path-input"
                                value={paneId === 'left' ? pathDraftLeft : pathDraftRight}
                                oninput={(e) => onPathInput(paneId, e.currentTarget.value)}
                                onkeydown={(e) => onPathKeydown(paneId, e)}
                                onfocus={() => activePane.set(paneId)}
                                onblur={() => setTimeout(() => closeSuggest(paneId), 150)}
                                spellcheck="false"
                                autocomplete="off"
                                aria-label="Current path"
                            />
                            <button
                                class="fm-up"
                                title="Refresh"
                                onclick={() => loadDir(paneId)}
                            >
                                <RotateCw class="fm-ico" />
                            </button>
                            <button
                                class="fm-up"
                                title={p.showHidden ? 'Hide hidden files' : 'Show hidden files'}
                                onclick={() => setShowHidden(paneId, !p.showHidden)}
                            >
                                {#if p.showHidden}<Eye class="fm-ico" />{:else}<EyeOff class="fm-ico" />{/if}
                            </button>
                            <button
                                class="fm-up"
                                class:is-on={$showFolderSizes}
                                title={$showFolderSizes ? 'Hide folder sizes' : 'Show folder sizes'}
                                onclick={() => setShowFolderSizes(!$showFolderSizes)}
                            >
                                <Sigma class="fm-ico" />
                            </button>

                            {#if pathSugOpen[paneId] && pathSug[paneId].length}
                                <div class="fm-suggest" role="listbox" aria-label="Folder suggestions">
                                    {#each pathSug[paneId] as sg, i (sg)}
                                        <button
                                            type="button"
                                            class="fm-suggest-row"
                                            class:is-active={pathSugIndex[paneId] === i}
                                            role="option"
                                            aria-selected={pathSugIndex[paneId] === i}
                                            onmousedown={(e) => {
                                                e.preventDefault();
                                                chooseSuggest(paneId, sg);
                                            }}
                                            onmouseenter={() => (pathSugIndex[paneId] = i)}
                                        >
                                            <Folder class="fm-suggest-ico" />
                                            <span class="fm-suggest-name">{leafName(sg)}</span>
                                            <span class="fm-suggest-path">{sg}</span>
                                        </button>
                                    {/each}
                                </div>
                            {/if}
                        </div>

                        <!-- Drive switcher -->
                        <div class="fm-drives">
                            {#each drives as drive (drive)}
                                <button
                                    class="fm-drive"
                                    class:is-current={p.path.toUpperCase().startsWith(drive.toUpperCase())}
                                    onclick={() => switchDrive(paneId, drive)}
                                    title={drive}
                                >
                                    <HardDrive class="fm-drive-ico" />
                                    <span>{drive.replace(/\\$/, '')}</span>
                                </button>
                            {/each}
                        </div>

                        <!-- Breadcrumb -->
                        <div class="fm-crumbs">
                            {#each breadcrumbs(p.path) as crumb, i (crumb.full)}
                                {#if i > 0}<ChevronRight class="fm-crumb-sep" />{/if}
                                <button class="fm-crumb" onclick={() => navigateTo(paneId, crumb.full)}>
                                    {crumb.label}
                                </button>
                            {/each}
                        </div>

                        <!-- Sort header -->
                        <div class="fm-sorthead">
                            {#each sortOptions as opt (opt.value)}
                                <button
                                    class="fm-sortbtn"
                                    class:is-on={p.sortBy === opt.value}
                                    onclick={() => setSort(paneId, opt.value)}
                                >
                                    {opt.label}
                                </button>
                            {/each}
                            <span class="fm-count">
                                {p.entries.length}{p.truncated ? ` of ${p.total}` : ''} items
                            </span>
                        </div>

                        <!-- File list -->
                        <div
                            class="fm-list"
                            bind:this={scrollEls[paneId]}
                            role="listbox"
                            tabindex="0"
                            aria-label={`${paneId} pane files`}
                            aria-multiselectable="true"
                            onkeydown={(e) => onPaneKeydown(paneId, e)}
                            onfocus={() => activePane.set(paneId)}
                        >
                            {#if p.loading}
                                <div class="fm-state"><EmptyState icon={Folder} title="Loading…" variant="compact" /></div>
                            {:else if p.error}
                                <div class="fm-state">
                                    <ErrorState
                                        title="Could not open this folder"
                                        description={p.error}
                                        retry={() => loadDir(paneId)}
                                    />
                                </div>
                            {:else if p.entries.length === 0}
                                <div class="fm-state">
                                    <EmptyState icon={Folder} title="This folder is empty" variant="compact" />
                                </div>
                            {:else}
                                {#each p.entries as entry, index (entry.path)}
                                    {@const Icon = iconForEntry(entry)}
                                    {@const isSel = p.selected.includes(entry.path)}
                                    <!-- Keyboard is handled at the listbox container (arrows / Enter
                                         drive focusIndex); rows are tabindex=-1 options, so the row's
                                         own click/dblclick need no per-row key handler. -->
                                    <!-- svelte-ignore a11y_click_events_have_key_events -->
                                    <div
                                        class="fm-row"
                                        class:is-selected={isSel}
                                        class:is-focus={p.focusIndex === index}
                                        class:is-hidden-file={entry.hidden}
                                        data-row-index={index}
                                        role="option"
                                        aria-selected={isSel}
                                        tabindex="-1"
                                        draggable="true"
                                        onclick={(e) => onRowClick(paneId, index, e)}
                                        ondblclick={() => onRowDblClick(paneId, entry)}
                                        ondragstart={(e) => onRowDragStart(paneId, entry, e)}
                                        ondragend={onRowDragEnd}
                                    >
                                        <span class="fm-row-icon" class:is-dir={entry.isDir}>
                                            <Icon class="fm-ico" />
                                        </span>
                                        <span class="fm-row-name" title={entry.name}>{entry.name}</span>
                                        <span class="fm-row-size">
                                            {#if !entry.isDir}
                                                {formatBytes(entry.size)}
                                            {:else if !$showFolderSizes}
                                                <span class="fm-size-dim">—</span>
                                            {:else if $folderSizes[entry.path]?.status === 'done'}
                                                {formatBytes($folderSizes[entry.path]?.bytes ?? 0)}
                                            {:else if $folderSizes[entry.path]?.status === 'error'}
                                                <span class="fm-size-dim">—</span>
                                            {:else}
                                                <Loader2 class="fm-size-spin" />
                                            {/if}
                                        </span>
                                        <span class="fm-row-mod">{formatModified(entry.modifiedMs)}</span>
                                    </div>
                                {/each}
                            {/if}
                        </div>
                    </section>

                    {#if paneId === 'left'}
                        <!-- Draggable vertical divider -->
                        <div
                            class="fm-divider"
                            class:is-dragging={dividerDragging}
                            role="separator"
                            aria-orientation="vertical"
                            aria-label="Resize panes"
                            onpointerdown={onDividerDown}
                            onpointermove={onDividerMove}
                            onpointerup={onDividerUp}
                        ></div>
                    {/if}
                {/each}
            </div>

            <!-- Progress bar (copy / move / backup) -->
            {#if runningOp}
                <div class="fm-progress">
                    <div class="fm-progress-info">
                        <span class="fm-progress-label">
                            {#if progress}
                                {progress.done} / {progress.total}
                            {:else}
                                Working…
                            {/if}
                        </span>
                        <span class="fm-progress-path" title={progress?.currentPath ?? ''}>
                            {progress?.currentPath ?? ''}
                        </span>
                        <Button variant="ghost" size="sm" icon={X} onclick={cancelOp}>Cancel</Button>
                    </div>
                    <div class="fm-progress-track">
                        <div
                            class="fm-progress-fill"
                            style={`width: ${progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0}%`}
                        ></div>
                    </div>
                </div>
            {/if}


        </div>
    {/snippet}
</ToolPage>

<!-- Rename dialog -->
{#if renameOpen}
    <div class="fm-modal-backdrop" role="presentation" onclick={() => (renameOpen = false)}>
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="fm-modal" role="dialog" aria-modal="true" aria-label="Rename" tabindex="-1" onclick={(e) => e.stopPropagation()}>
            <h3 class="fm-modal-title">Rename</h3>
            <!-- svelte-ignore a11y_autofocus -->
            <input
                class="fm-modal-input"
                bind:value={renameValue}
                onkeydown={(e) => { if (e.key === 'Enter') commitRename(); if (e.key === 'Escape') renameOpen = false; }}
                autofocus
                spellcheck="false"
            />
            <div class="fm-modal-actions">
                <Button variant="ghost" size="sm" onclick={() => (renameOpen = false)}>Cancel</Button>
                <Button variant="primary" size="sm" onclick={commitRename}>Rename</Button>
            </div>
        </div>
    </div>
{/if}

<!-- New-folder dialog -->
{#if newFolderOpen}
    <div class="fm-modal-backdrop" role="presentation" onclick={() => (newFolderOpen = false)}>
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="fm-modal" role="dialog" aria-modal="true" aria-label="New folder" tabindex="-1" onclick={(e) => e.stopPropagation()}>
            <h3 class="fm-modal-title">New folder</h3>
            <!-- svelte-ignore a11y_autofocus -->
            <input
                class="fm-modal-input"
                bind:value={newFolderName}
                placeholder="Folder name"
                onkeydown={(e) => { if (e.key === 'Enter') commitNewFolder(); if (e.key === 'Escape') newFolderOpen = false; }}
                autofocus
                spellcheck="false"
            />
            <div class="fm-modal-actions">
                <Button variant="ghost" size="sm" onclick={() => (newFolderOpen = false)}>Cancel</Button>
                <Button variant="primary" size="sm" onclick={commitNewFolder}>Create</Button>
            </div>
        </div>
    </div>
{/if}

<!-- Backup dialog -->
{#if backupOpen}
    <div class="fm-modal-backdrop" role="presentation" onclick={() => (backupOpen = false)}>
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="fm-modal fm-modal-wide" role="dialog" aria-modal="true" aria-label="Backup" tabindex="-1" onclick={(e) => e.stopPropagation()}>
            <h3 class="fm-modal-title">Backup folder</h3>
            <label class="fm-field">
                <span class="fm-field-label">Source</span>
                <input class="fm-modal-input" value={backupSource} readonly spellcheck="false" />
            </label>
            <label class="fm-field">
                <span class="fm-field-label">Destination</span>
                <div class="fm-field-row">
                    <input class="fm-modal-input" bind:value={backupDest} spellcheck="false" />
                    <Button variant="secondary" size="sm" onclick={pickBackupDest}>Browse…</Button>
                </div>
            </label>
            <label class="fm-mode">
                <input type="radio" name="backup-mode" checked={!backupMirror} onchange={() => (backupMirror = false)} />
                <span><strong>Copy</strong> — add and update files, leave existing extras in the destination.</span>
            </label>
            <label class="fm-mode">
                <input type="radio" name="backup-mode" checked={backupMirror} onchange={() => (backupMirror = true)} />
                <span><strong>Mirror</strong> — make the destination an exact replica; <em>deletes extras</em> in the destination.</span>
            </label>
            <div class="fm-modal-actions">
                <Button variant="ghost" size="sm" onclick={() => (backupOpen = false)}>Cancel</Button>
                <Button variant={backupMirror ? 'danger' : 'primary'} size="sm" onclick={runBackup}>
                    {backupMirror ? 'Mirror' : 'Copy'}
                </Button>
            </div>
        </div>
    </div>
{/if}

<style>
    .fm-shell {
        display: flex;
        flex-direction: column;
        gap: 12px;
        height: calc(100vh - 200px);
        min-height: 420px;
    }

    /* ─── Panes + divider ─────────────────────────────────────────── */
    .fm-panes {
        flex: 1;
        min-height: 0;
        display: grid;
        gap: 0;
    }
    .fm-pane {
        display: flex;
        flex-direction: column;
        min-width: 0;
        min-height: 0;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel);
        overflow: hidden;
        transition: border-color var(--dur-micro) var(--ease-out);
    }
    .fm-pane.is-active {
        border-color: var(--color-border-strong);
    }
    .fm-pane.is-drop {
        border-color: var(--color-accent);
        box-shadow: inset 0 0 0 1px var(--color-accent);
    }

    .fm-divider {
        cursor: col-resize;
        display: flex;
        align-items: stretch;
        justify-content: center;
    }
    .fm-divider::after {
        content: '';
        width: 1px;
        background: var(--color-border);
        transition: background-color var(--dur-micro) var(--ease-out);
    }
    .fm-divider:hover::after,
    .fm-divider.is-dragging::after {
        width: 2px;
        background: var(--color-accent);
    }

    /* ─── Path bar ────────────────────────────────────────────────── */
    .fm-pathbar {
        position: relative;
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 8px 8px 6px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    /* Path autocomplete dropdown */
    .fm-suggest {
        position: absolute;
        top: calc(100% - 2px);
        left: 8px;
        right: 8px;
        z-index: 30;
        max-height: 260px;
        overflow-y: auto;
        display: flex;
        flex-direction: column;
        padding: 4px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel);
        box-shadow: 0 10px 28px rgba(0, 0, 0, 0.35);
    }
    .fm-suggest-row {
        display: flex;
        align-items: center;
        gap: 8px;
        width: 100%;
        padding: 6px 8px;
        border: none;
        border-radius: var(--radius-control);
        background: transparent;
        color: var(--color-text);
        font-size: 12.5px;
        text-align: left;
        cursor: pointer;
        min-width: 0;
    }
    .fm-suggest-row.is-active {
        background: color-mix(in srgb, var(--color-accent) 16%, transparent);
    }
    :global(.fm-suggest-ico) {
        flex: none;
        width: 15px;
        height: 15px;
        color: var(--color-accent);
    }
    .fm-suggest-name {
        flex: none;
        font-weight: 600;
        white-space: nowrap;
    }
    .fm-suggest-path {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        text-align: right;
        font-size: 11px;
        color: var(--color-muted);
    }
    .fm-up {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 28px;
        height: 28px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
        cursor: pointer;
        transition: background-color var(--dur-micro) var(--ease-out);
    }
    .fm-up:hover:not(:disabled) {
        background: var(--color-panel-3);
        color: var(--color-text);
    }
    .fm-up:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }
    .fm-up.is-on {
        color: var(--color-accent);
        border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
        background: color-mix(in srgb, var(--color-accent) 12%, var(--color-panel-2));
    }
    /* Folder-size column states */
    .fm-size-dim {
        color: var(--color-muted);
    }
    :global(.fm-size-spin) {
        width: 12px;
        height: 12px;
        color: var(--color-muted);
        animation: fm-size-spin 0.8s linear infinite;
    }
    @keyframes fm-size-spin {
        to {
            transform: rotate(360deg);
        }
    }
    @media (prefers-reduced-motion: reduce) {
        :global(.fm-size-spin) {
            animation: none;
        }
    }
    .fm-path-input {
        flex: 1;
        min-width: 0;
        height: 28px;
        padding: 0 10px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-size: 12px;
        font-family: var(--font-mono, monospace);
    }
    .fm-path-input:focus {
        outline: none;
        border-color: var(--color-accent);
    }

    /* ─── Drive switcher ──────────────────────────────────────────── */
    .fm-drives {
        display: flex;
        flex-wrap: wrap;
        gap: 4px;
        padding: 6px 8px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .fm-drive {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        height: 24px;
        padding: 0 8px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
        font-size: 11.5px;
        cursor: pointer;
        transition: background-color var(--dur-micro) var(--ease-out);
    }
    .fm-drive:hover {
        background: var(--color-panel-3);
        color: var(--color-text);
    }
    .fm-drive.is-current {
        background: var(--color-panel-2);
        border-color: var(--color-accent);
        color: var(--color-text);
    }
    .fm-pane :global(.fm-drive-ico) {
        width: 13px;
        height: 13px;
    }

    /* ─── Breadcrumb ──────────────────────────────────────────────── */
    .fm-crumbs {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 2px;
        padding: 6px 10px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        min-height: 30px;
    }
    .fm-crumb {
        padding: 2px 6px;
        border: none;
        border-radius: var(--radius-control);
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12px;
        cursor: pointer;
    }
    .fm-crumb:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .fm-pane :global(.fm-crumb-sep) {
        width: 13px;
        height: 13px;
        color: var(--color-muted);
        flex: none;
    }

    /* ─── Sort header ─────────────────────────────────────────────── */
    .fm-sorthead {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 5px 10px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .fm-sortbtn {
        padding: 2px 8px;
        border: none;
        border-radius: var(--radius-control);
        background: transparent;
        color: var(--color-muted);
        font-size: 11.5px;
        font-weight: 500;
        cursor: pointer;
    }
    .fm-sortbtn:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .fm-sortbtn.is-on {
        color: var(--color-accent);
    }
    .fm-count {
        margin-left: auto;
        font-size: 11px;
        color: var(--color-muted);
        font-variant-numeric: tabular-nums;
    }

    /* ─── File list ───────────────────────────────────────────────── */
    .fm-list {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        padding: 4px;
        outline: none;
    }
    .fm-list:focus-visible {
        box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--color-accent) 40%, transparent);
        border-radius: var(--radius-control);
    }
    .fm-state {
        padding: 24px 12px;
    }

    .fm-row {
        position: relative;
        display: grid;
        grid-template-columns: 22px 1fr auto auto;
        align-items: center;
        gap: 10px;
        padding: 5px 10px 5px 12px;
        border-radius: var(--radius-control);
        cursor: default;
        user-select: none;
    }
    .fm-row:hover:not(.is-selected) {
        background: color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    /* Canonical selected look: neutral panel-2 bg + 3px accent left strip. */
    .fm-row.is-selected {
        background: var(--color-panel-2);
    }
    .fm-row.is-selected::before {
        content: '';
        position: absolute;
        left: 3px;
        top: 6px;
        bottom: 6px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .fm-row.is-focus {
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-accent) 35%, transparent);
    }
    .fm-row.is-hidden-file {
        opacity: 0.55;
    }
    .fm-row-icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        color: var(--color-text-secondary);
    }
    .fm-row-icon.is-dir {
        color: var(--color-accent);
    }
    .fm-pane :global(.fm-ico) {
        width: 15px;
        height: 15px;
    }
    .fm-row-name {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 13px;
        color: var(--color-text);
    }
    .fm-row-size,
    .fm-row-mod {
        font-size: 11.5px;
        color: var(--color-muted);
        font-variant-numeric: tabular-nums;
        white-space: nowrap;
    }
    .fm-row-mod {
        min-width: 92px;
        text-align: right;
    }

    /* ─── Progress ────────────────────────────────────────────────── */
    .fm-progress {
        display: flex;
        flex-direction: column;
        gap: 6px;
        padding: 8px 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel);
    }
    .fm-progress-info {
        display: flex;
        align-items: center;
        gap: 12px;
    }
    .fm-progress-label {
        font-size: 12px;
        font-weight: 500;
        color: var(--color-text);
        font-variant-numeric: tabular-nums;
        flex: none;
    }
    .fm-progress-path {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 11.5px;
        color: var(--color-muted);
    }
    .fm-progress-track {
        height: 4px;
        border-radius: 999px;
        background: var(--color-panel-3);
        overflow: hidden;
    }
    .fm-progress-fill {
        height: 100%;
        background: var(--color-accent);
        border-radius: 999px;
        transition: width 120ms var(--ease-out);
    }

    /* ─── Action bar ──────────────────────────────────────────────── */
    .fm-actions {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        justify-content: flex-end;
        gap: 8px;
        padding: 0 0 10px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .fm-actions-sep {
        width: 1px;
        height: 20px;
        background: var(--color-border);
        margin: 0 2px;
    }

    /* ─── Modals ──────────────────────────────────────────────────── */
    .fm-modal-backdrop {
        position: fixed;
        inset: 0;
        z-index: 200;
        display: flex;
        align-items: center;
        justify-content: center;
        background: color-mix(in srgb, var(--color-bg) 60%, transparent);
        backdrop-filter: blur(2px);
    }
    .fm-modal {
        width: min(420px, calc(100vw - 48px));
        display: flex;
        flex-direction: column;
        gap: 12px;
        padding: 18px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel);
        box-shadow: var(--shadow-lg, 0 12px 40px rgba(0, 0, 0, 0.4));
    }
    .fm-modal-wide {
        width: min(540px, calc(100vw - 48px));
    }
    .fm-modal-title {
        margin: 0;
        font-size: 15px;
        font-weight: 600;
        color: var(--color-text);
    }
    .fm-modal-input {
        width: 100%;
        height: 32px;
        padding: 0 10px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-size: 13px;
    }
    .fm-modal-input:focus {
        outline: none;
        border-color: var(--color-accent);
    }
    .fm-modal-actions {
        display: flex;
        justify-content: flex-end;
        gap: 8px;
        margin-top: 4px;
    }
    .fm-field {
        display: flex;
        flex-direction: column;
        gap: 5px;
    }
    .fm-field-label {
        font-size: 12px;
        font-weight: 500;
        color: var(--color-text-secondary);
    }
    .fm-field-row {
        display: flex;
        gap: 8px;
    }
    .fm-field-row .fm-modal-input {
        flex: 1;
    }
    .fm-mode {
        display: flex;
        align-items: flex-start;
        gap: 8px;
        font-size: 12.5px;
        line-height: 1.45;
        color: var(--color-text-secondary);
        cursor: pointer;
    }
    .fm-mode input {
        margin-top: 2px;
        accent-color: var(--color-accent);
    }
    .fm-mode strong {
        color: var(--color-text);
    }
</style>
