<script lang="ts">
    /*
      Quality Pass Wave 1 — Phase 1 — DiffMerge rewrite (2026-05-28).
      Items shipped here:
        DM-1: Modernize to ToolPage / Tabs / Button / Textarea / EmptyState /
              LoadingState / ErrorState kit primitives (was the only File-tool
              outlier still using raw flex divs and inline tab buttons).
        DM-2: Side-by-side diff view as a toggle next to the unified view —
              every free competitor (WinMerge, Meld, Beyond Compare, VS Code)
              has this; we shouldn't be the odd one out.
        DM-3: Third tab "File vs File" — pick any two text files, diff them
              directly. Previously diffs required a folder pair, which made
              the tool useless for the very common "compare two configs"
              workflow.

      DM-4 (folder sync actions) lands in a follow-up because it needs new
      backend commands; this file is frontend-only.
    */
    import { open, save } from '@tauri-apps/plugin-dialog';
    import { writeTextFile } from '@tauri-apps/plugin-fs';
    import { get } from 'svelte/store';
    import { confirm } from '$lib/stores/confirmDialog';
    import {
        FolderOpen,
        FileCode2,
        GitCompareArrows,
        GitMerge,
        Search,
        X,
        Copy,
        Save,
        AlignJustify,
        Columns2,
    } from '@lucide/svelte';
    import {
        folderDiff,
        fileUnifiedDiff,
        threeWayMerge,
        diffReadText,
        cancelDiffOperation,
        formatSize,
        syncFolders,
        type SyncMode,
        type DiffEntry,
    } from '$lib/stores/diffMerge';
    import {
        diffTab,
        diffBusy,
        diffFileLoading,
        diffLeftDir,
        diffRightDir,
        diffResult,
        diffExpanded,
        diffText as diffTextStore,
        diffLoading,
        diffFilter,
        diffCurrentOpId,
        diffFileLeft,
        diffFileRight,
        diffFileText,
        diffViewMode,
        diffSyncResult,
        diffSyncing,
        mergeBase,
        mergeOurs,
        mergeTheirs,
        mergeResultState,
    } from '$lib/stores/diffMergeState';
    import {
        Button,
        Card,
        Tabs,
        Textarea,
        ToolPage,
        EmptyState,
        ErrorState,
        LoadingState,
    } from '$lib/ui';

    // ─── Tab state (folder / file / merge) ─────────────────────────
    // Wave 3.2 background-task continuity: hydrate from per-tool store
    // on mount, sync back via $effect. The 'file' tab is new in Phase
    // 1 (DM-3) — older stored values default safely to 'folder'.
    type TabId = 'folder' | 'file' | 'merge';
    let tab = $state<TabId>(get(diffTab) as TabId);
    $effect(() => { diffTab.set(tab); });
    let busy = $derived($diffBusy);

    // Inline message banner (kept as a state for Wave 2.5 cancel + Wave
    // 7.6's flash polish — would normally be a toast but Folder Diff /
    // Merge errors are tied to a specific surface, so the inline banner
    // gives the right "where to look" cue).
    let message = $state<{ kind: 'ok' | 'err'; text: string } | null>(null);
    function flash(kind: 'ok' | 'err', text: string) {
        message = { kind, text };
        setTimeout(() => (message = null), 4500);
    }

    // ─── Folder diff state ────────────────────────────────────────
    let leftDir = $state(get(diffLeftDir));
    $effect(() => { diffLeftDir.set(leftDir); });
    let rightDir = $state(get(diffRightDir));
    $effect(() => { diffRightDir.set(rightDir); });
    let result = $derived($diffResult);
    let expanded = $derived($diffExpanded);
    let diffText = $derived($diffTextStore);
    let diffLoadingLocal = $derived($diffLoading);
    let filter = $state<'all' | 'added' | 'removed' | 'modified'>(get(diffFilter));
    $effect(() => { diffFilter.set(filter); });

    // ─── DM-2 side-by-side toggle ─────────────────────────────────
    // 'unified' = classic patch view (+/-/space). 'split' = WinMerge-
    // style two-column. Persisted via the per-tool store so the user's
    // preference (and the rendered file diff below) survive navigation.
    let diffView = $state<'unified' | 'split'>(get(diffViewMode));
    $effect(() => { diffViewMode.set(diffView); });

    // ─── File-vs-file (DM-3) state ────────────────────────────────
    // Background-task continuity (2026-06-12): the folder tab already
    // persisted; the file tab now hydrates from the store on mount and
    // mirrors back via $effect so a computed file diff survives a remount.
    let fileLeft = $state(get(diffFileLeft));
    $effect(() => { diffFileLeft.set(fileLeft); });
    let fileRight = $state(get(diffFileRight));
    $effect(() => { diffFileRight.set(fileRight); });
    let fileDiffText = $derived($diffFileText);
    // Transient — local only, like folder `busy`/`diffLoadingLocal`.
    let fileDiffLoading = $derived($diffFileLoading);

    // ─── Cancel token (Wave 2.5) ──────────────────────────────────
    let currentDiffOpId = $derived($diffCurrentOpId);

    async function pickDir(which: 'left' | 'right') {
        if (get(diffBusy)) return;
        const picked = await open({ directory: true, multiple: false, title: `Choose the ${which} folder` });
        if (picked && !Array.isArray(picked)) {
            if (which === 'left') leftDir = picked;
            else rightDir = picked;
        }
    }

    async function pickFile(which: 'left' | 'right') {
        if (get(diffBusy)) return;
        const picked = await open({ multiple: false, title: `Choose the ${which} file` });
        if (picked && !Array.isArray(picked)) {
            if (which === 'left') fileLeft = picked;
            else fileRight = picked;
        }
    }

    async function runFolderDiff() {
        if (!leftDir || !rightDir || get(diffBusy)) return;
        diffBusy.set(true);
        diffResult.set(null);
        diffExpanded.set(null);
        const opId = `diff-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
        diffCurrentOpId.set(opId);
        try {
            diffResult.set(await folderDiff(leftDir, rightDir, opId));
        } catch (e) {
            const errStr = String(e);
            if (errStr.includes('Cancelled')) flash('err', 'Folder diff cancelled');
            else flash('err', errStr);
        } finally {
            diffCurrentOpId.set(null);
            diffBusy.set(false);
        }
    }

    async function cancelFolderDiff() {
        if (!currentDiffOpId) return;
        try { await cancelDiffOperation(currentDiffOpId); } catch { /* best-effort */ }
    }

    // Folder sync. Mirror overwrites and deletes for good, so it asks every
    // time, naming both folders.
    let syncing = $derived($diffSyncing);
    let syncResult = $derived($diffSyncResult);

    async function runSync(mode: SyncMode) {
        if (!leftDir || !rightDir || get(diffSyncing) || get(diffBusy)) return;
        if (mode === 'mirror_left_to_right') {
            const ok = await confirm(
                `Make “${rightDir}” an exact copy of “${leftDir}”? Files that differ are overwritten, and files that are only in “${rightDir}” are deleted permanently.`,
                { title: 'Mirror folders', kind: 'warning', confirmLabel: 'Mirror', danger: true },
            );
            if (!ok || get(diffSyncing) || get(diffBusy)) return;
        }
        diffSyncing.set(true);
        diffBusy.set(true);
        diffSyncResult.set(null);
        try {
            const syncResult = await syncFolders(leftDir, rightDir, mode);
            diffSyncResult.set(syncResult);
            flash(
                syncResult.failedCount > 0 ? 'err' : 'ok',
                `Sync done · ${syncResult.copiedCount} copied · ${syncResult.deletedCount} deleted${
                    syncResult.failedCount > 0 ? ` · ${syncResult.failedCount} failed` : ''
                }`,
            );
        } catch (e) {
            flash('err', String(e));
        } finally {
            diffSyncing.set(false);
            diffBusy.set(false);
        }
    }

    async function runFileDiff() {
        if (!fileLeft || !fileRight || get(diffBusy)) return;
        diffBusy.set(true);
        diffFileLoading.set(true);
        diffFileText.set('');
        try {
            const fileDiffText = await fileUnifiedDiff(fileLeft, fileRight);
            diffFileText.set(fileDiffText);
            if (!fileDiffText.trim()) flash('ok', 'Files are identical.');
        } catch (e) {
            flash('err', String(e));
        } finally {
            diffFileLoading.set(false);
            diffBusy.set(false);
        }
    }

    async function toggleEntry(entry: DiffEntry) {
        if (expanded === entry.path) { diffExpanded.set(null); return; }
        diffExpanded.set(entry.path);
        diffTextStore.set('');
        if (entry.status !== 'modified' || !entry.isText) return;
        diffLoading.set(true);
        try {
            const text = await fileUnifiedDiff(`${leftDir}/${entry.path}`, `${rightDir}/${entry.path}`);
            if (get(diffExpanded) === entry.path) diffTextStore.set(text);
        } catch (e) {
            if (get(diffExpanded) === entry.path) diffTextStore.set(String(e));
        } finally {
            if (get(diffExpanded) === entry.path) diffLoading.set(false);
        }
    }

    const filtered = $derived(
        result ? result.entries.filter((e) => filter === 'all' || e.status === filter) : [],
    );
    // Elegance pass (2026-06-05): semantic tokens, not hardcoded hex, so the
    // status dots track the active theme (success/error/warning) like the rest
    // of the kit. Behaviour identical — same three states, same mapping.
    const statusColor = (s: string) =>
        s === 'added'
            ? 'var(--color-success)'
            : s === 'removed'
              ? 'var(--color-error)'
              : 'var(--color-warning)';

    // ─── Side-by-side parser (DM-2) ───────────────────────────────
    // Walks a unified diff and pairs `-` and `+` runs into two columns.
    // Pure function, no streaming — fine for human-scale file diffs.
    // The classic algorithm: buffer consecutive `-` lines and `+` lines,
    // then flush them paired up when we hit a context/header line.
    type SbsRow = { left: string | null; right: string | null; kind: 'context' | 'change' | 'header' };
    function parseSideBySide(unified: string): SbsRow[] {
        const rows: SbsRow[] = [];
        let pendMinus: string[] = [];
        let pendPlus: string[] = [];
        const flush = () => {
            const max = Math.max(pendMinus.length, pendPlus.length);
            for (let i = 0; i < max; i++) {
                rows.push({ left: pendMinus[i] ?? null, right: pendPlus[i] ?? null, kind: 'change' });
            }
            pendMinus = [];
            pendPlus = [];
        };
        for (const line of unified.split('\n')) {
            if (line.startsWith('+++') || line.startsWith('---') || line.startsWith('@@')) {
                flush();
                rows.push({ left: line, right: line, kind: 'header' });
            } else if (line.startsWith('-')) {
                pendMinus.push(line.slice(1));
            } else if (line.startsWith('+')) {
                pendPlus.push(line.slice(1));
            } else {
                flush();
                const content = line.startsWith(' ') ? line.slice(1) : line;
                rows.push({ left: content, right: content, kind: 'context' });
            }
        }
        flush();
        return rows;
    }

    // Diff line coloring for the unified view.
    function diffLineClass(line: string): string {
        if (line.startsWith('+') && !line.startsWith('+++')) return 'diff-add';
        if (line.startsWith('-') && !line.startsWith('---')) return 'diff-del';
        if (line.startsWith('@@')) return 'diff-hunk';
        return 'diff-ctx';
    }

    // ─── 3-way merge state ────────────────────────────────────────
    let base = $state(get(mergeBase));
    $effect(() => { mergeBase.set(base); });
    let ours = $state(get(mergeOurs));
    $effect(() => { mergeOurs.set(ours); });
    let theirs = $state(get(mergeTheirs));
    $effect(() => { mergeTheirs.set(theirs); });
    let merge = $derived($mergeResultState);

    async function loadInto(target: 'base' | 'ours' | 'theirs') {
        if (get(diffBusy)) return;
        const picked = await open({ multiple: false, title: `Load the ${target} file` });
        if (!picked || Array.isArray(picked)) return;
        try {
            const text = await diffReadText(picked);
            if (target === 'base') { base = text; mergeBase.set(text); }
            else if (target === 'ours') { ours = text; mergeOurs.set(text); }
            else { theirs = text; mergeTheirs.set(text); }
        } catch (e) { flash('err', String(e)); }
    }

    async function runMerge() {
        if (get(diffBusy)) return;
        diffBusy.set(true);
        mergeResultState.set(null);
        try {
            const merge = await threeWayMerge(base, ours, theirs);
            mergeResultState.set(merge);
            flash(
                merge.conflicts ? 'err' : 'ok',
                merge.conflicts ? 'Merged with conflicts — resolve the marked regions.' : 'Merged cleanly.',
            );
        } catch (e) {
            flash('err', String(e));
        } finally {
            diffBusy.set(false);
        }
    }

    async function copyMerged() {
        if (!merge) return;
        try { await navigator.clipboard.writeText(merge.merged); flash('ok', 'Copied'); }
        catch { flash('err', 'Could not copy'); }
    }
    async function saveMerged() {
        if (!merge) return;
        const path = await save({ title: 'Save merged result', defaultPath: 'merged.txt' });
        if (!path) return;
        try { await writeTextFile(path, merge.merged); flash('ok', 'Saved'); }
        catch (e) { flash('err', String(e)); }
    }

    // Tabs primitive metadata.
    const tabs = [
        { id: 'folder', label: 'Folder vs Folder', icon: GitCompareArrows },
        { id: 'file', label: 'File vs File', icon: FileCode2 },
        { id: 'merge', label: '3-Way Merge', icon: GitMerge },
    ];
</script>

<ToolPage
    icon={GitCompareArrows}
    iconTint="#64748b"
    title="Compare and merge — no git required"
    description="Side-by-side or unified diff between two folders, two files, or a 3-way merge — everything happens locally."
    width="wide"
    fill={false}
>
    {#snippet actions()}
        <Tabs {tabs} active={tab} onChange={(id) => (tab = id as TabId)} size="sm" ariaLabel="Diff mode" />
    {/snippet}

    {#if message}
        {#if message.kind === 'err'}
            <ErrorState title="Something went wrong" description={message.text} />
        {:else}
            <Card>
                <div class="dm-ok">{message.text}</div>
            </Card>
        {/if}
    {/if}

    {#if tab === 'folder'}
        <!-- ── Folder vs Folder ─────────────────────────────────── -->
        <Card>
            <div class="dm-section-head">
                <span class="dm-step" aria-hidden="true">1</span>
                <h2 class="dm-section-title">Choose folders</h2>
            </div>
            <div class="dm-dual-pickers">
                {#each [['left', leftDir, 'Left (original)'], ['right', rightDir, 'Right (changed)']] as [side, dir, label]}
                    <label class="dm-picker-field">
                        <span class="dm-picker-label">{label} folder</span>
                        <button
                            type="button"
                            onclick={() => pickDir(side as 'left' | 'right')}
                            disabled={busy}
                            class="dm-picker-btn"
                            title={dir || 'Choose folder'}
                        >
                            <FolderOpen class="dm-picker-ico" />
                            <span class="dm-picker-path">{dir || 'Choose folder…'}</span>
                        </button>
                    </label>
                {/each}
            </div>
            <div class="dm-actions">
                <Button
                    variant="primary"
                    icon={Search}
                    onclick={runFolderDiff}
                    disabled={!leftDir || !rightDir || busy}
                >
                    {busy ? 'Comparing…' : 'Compare folders'}
                </Button>
                {#if busy}
                    <Button variant="ghost" icon={X} onclick={cancelFolderDiff}>Cancel</Button>
                {/if}
            </div>
        </Card>

        {#if busy && !result}
            <Card>
                <LoadingState
                    variant="block"
                    label="Comparing folders…"
                    subLabel="Walking the trees, byte-comparing files. Cancel anytime — latency is one entry or one byte-compare."
                />
            </Card>
        {/if}

        {#if result}
            <Card>
                <div class="dm-section-head">
                    <span class="dm-step" aria-hidden="true">2</span>
                    <h2 class="dm-section-title">Review differences</h2>
                </div>
                <!-- User feedback (2026-05-29): "added/removed/modified"
                     sounded like a file operation happened. Diff is
                     read-only — nothing on disk changes. Re-worded for
                     clarity around what each count means. -->
                <div class="dm-stats-row">
                    <span class="dm-stat dm-stat-add" title="Files present in the right folder but not in the left">
                        {result.added} only in right
                    </span>
                    <span class="dm-stat dm-stat-rem" title="Files present in the left folder but not in the right">
                        {result.removed} only in left
                    </span>
                    <span class="dm-stat dm-stat-mod" title="Files in both folders with different content">
                        {result.modified} different
                    </span>
                    <span class="dm-stat dm-stat-id" title="Files identical in both folders">
                        {result.identical} identical
                    </span>
                    <div class="dm-filter-row">
                        {#each [
                            ['all', 'All'],
                            ['added', 'Only in right'],
                            ['removed', 'Only in left'],
                            ['modified', 'Different'],
                        ] as [value, label]}
                            <button
                                type="button"
                                onclick={() => (filter = value as typeof filter)}
                                class="dm-filter-btn {filter === value ? 'is-active' : ''}"
                            >{label}</button>
                        {/each}
                    </div>
                </div>

                <!-- Folder sync. Copy-only modes add what's missing; Mirror
                     overwrites and deletes, and asks first. -->
                <div class="dm-sync-row">
                    <span class="dm-step" aria-hidden="true">3</span>
                    <span class="dm-sync-label">Choose a sync action</span>
                    <Button
                        variant="ghost"
                        size="sm"
                        disabled={syncing || busy}
                        onclick={() => runSync('copy_left_to_right')}
                    >Copy missing L→R</Button>
                    <Button
                        variant="ghost"
                        size="sm"
                        disabled={syncing || busy}
                        onclick={() => runSync('copy_right_to_left')}
                    >Copy missing R→L</Button>
                    <Button
                        variant="ghost"
                        size="sm"
                        disabled={syncing || busy}
                        onclick={() => runSync('mirror_left_to_right')}
                    >Mirror L→R…</Button>
                    {#if syncing}
                        <span class="dm-sync-label">Working…</span>
                    {:else if syncResult}
                        <span class="dm-sync-label">
                            ✓ {syncResult.copiedCount} copied · {syncResult.deletedCount} deleted
                            {syncResult.failedCount > 0 ? `· ${syncResult.failedCount} failed` : ''}
                        </span>
                    {/if}
                </div>

                {#if filtered.length === 0}
                    <EmptyState
                        icon={GitCompareArrows}
                        title="No differences"
                        description={filter !== 'all'
                            ? `Nothing matches the "${filter}" filter — try All.`
                            : 'The folders match. Every file is identical by content.'}
                        variant="compact"
                    />
                {:else}
                    <!-- Side-by-side toggle (DM-2) only applies to file diffs
                         expanded below; keep it at the entry list header so it
                         affects every expansion consistently. -->
                    <div class="dm-view-toggle-row">
                        <span class="dm-view-toggle-label">View modified files as</span>
                        <Tabs
                            tabs={[
                                { id: 'unified', label: 'Unified', icon: AlignJustify },
                                { id: 'split', label: 'Split', icon: Columns2 },
                            ]}
                            active={diffView}
                            onChange={(id) => (diffView = id as 'unified' | 'split')}
                            size="sm"
                            ariaLabel="Diff display mode"
                        />
                    </div>
                    <div class="dm-entry-list">
                        {#each filtered as entry}
                            <div class="dm-entry" class:is-expanded={expanded === entry.path}>
                                <button
                                    type="button"
                                    onclick={() => toggleEntry(entry)}
                                    class="dm-entry-row"
                                    aria-expanded={expanded === entry.path}
                                >
                                    <span class="dm-entry-main">
                                        <span class="dm-entry-dot" style={`background:${statusColor(entry.status)}`}></span>
                                        <span class="dm-entry-path" title={entry.path}>{entry.path}</span>
                                    </span>
                                    <span class="dm-entry-meta">
                                        {#if entry.status === 'added'}only in right{:else if entry.status === 'removed'}only in left{:else if entry.status === 'modified'}different · {formatSize(entry.leftSize)} → {formatSize(entry.rightSize)}{:else}{entry.status}{/if}
                                    </span>
                                </button>
                                {#if expanded === entry.path}
                                    <div class="dm-entry-body">
                                        {#if entry.status !== 'modified'}
                                            <p class="dm-entry-note">
                                                {entry.status === 'added' ? 'Only in the right folder.' : 'Only in the left folder.'}
                                            </p>
                                        {:else if !entry.isText}
                                            <p class="dm-entry-note">Binary file — sizes differ.</p>
                                        {:else if diffLoadingLocal}
                                            <p class="dm-entry-note">Loading diff…</p>
                                        {:else if diffView === 'split'}
                                            {@const rows = parseSideBySide(diffText)}
                                            <div class="dm-sbs">
                                                {#each rows as row}
                                                    <div class="dm-sbs-row dm-sbs-{row.kind}">
                                                        <div class="dm-sbs-cell dm-sbs-left">{row.left ?? ' '}</div>
                                                        <div class="dm-sbs-cell dm-sbs-right">{row.right ?? ' '}</div>
                                                    </div>
                                                {/each}
                                            </div>
                                        {:else}
                                            <pre class="dm-uni">{#each diffText.split('\n') as line}<div class={diffLineClass(line)}>{line || ' '}</div>{/each}</pre>
                                        {/if}
                                    </div>
                                {/if}
                            </div>
                        {/each}
                    </div>
                {/if}
            </Card>
        {/if}
    {:else if tab === 'file'}
        <!-- ── File vs File (DM-3) ─────────────────────────────── -->
        <Card>
            <div class="dm-dual-pickers">
                {#each [['left', fileLeft, 'Left (original)'], ['right', fileRight, 'Right (changed)']] as [side, path, label]}
                    <label class="dm-picker-field">
                        <span class="dm-picker-label">{label} file</span>
                        <button
                            type="button"
                            onclick={() => pickFile(side as 'left' | 'right')}
                            disabled={busy}
                            class="dm-picker-btn"
                            title={path || 'Choose file'}
                        >
                            <FileCode2 class="dm-picker-ico" />
                            <span class="dm-picker-path">{path || 'Choose file…'}</span>
                        </button>
                    </label>
                {/each}
            </div>
            <div class="dm-actions">
                <Button
                    variant="primary"
                    icon={Search}
                    onclick={runFileDiff}
                    disabled={!fileLeft || !fileRight || busy}
                >
                    {fileDiffLoading ? 'Diffing…' : 'Compare files'}
                </Button>
                <div class="dm-view-toggle-row dm-view-toggle-inline">
                    <Tabs
                        tabs={[
                            { id: 'unified', label: 'Unified', icon: AlignJustify },
                            { id: 'split', label: 'Split', icon: Columns2 },
                        ]}
                        bind:active={diffView as string}
                        size="sm"
                        ariaLabel="Diff display mode"
                    />
                </div>
            </div>
        </Card>

        {#if fileDiffLoading}
            <Card>
                <LoadingState variant="block" label="Computing diff…" />
            </Card>
        {:else if fileDiffText}
            <Card>
                {#if diffView === 'split'}
                    {@const rows = parseSideBySide(fileDiffText)}
                    <div class="dm-sbs">
                        {#each rows as row}
                            <div class="dm-sbs-row dm-sbs-{row.kind}">
                                <div class="dm-sbs-cell dm-sbs-left">{row.left ?? ' '}</div>
                                <div class="dm-sbs-cell dm-sbs-right">{row.right ?? ' '}</div>
                            </div>
                        {/each}
                    </div>
                {:else}
                    <pre class="dm-uni">{#each fileDiffText.split('\n') as line}<div class={diffLineClass(line)}>{line || ' '}</div>{/each}</pre>
                {/if}
            </Card>
        {:else if !fileLeft || !fileRight}
            <EmptyState
                icon={FileCode2}
                title="Pick two files"
                description="Compare any two text files head-to-head, side-by-side or unified."
                variant="dashed"
            />
        {/if}
    {:else}
        <!-- ── 3-way merge ─────────────────────────────────────── -->
        <Card>
            <p class="dm-merge-blurb">
                Merge two sets of changes against a common <b>base</b>. Non-overlapping edits merge automatically; overlapping ones become <code>&lt;&lt;&lt;&lt;&lt;&lt;&lt;</code> conflict markers to resolve.
            </p>
            <div class="dm-merge-grid">
                {#each [['base', 'Base (common ancestor)'], ['ours', 'Ours'], ['theirs', 'Theirs']] as [key, label]}
                    <div class="dm-merge-pane">
                        <div class="dm-merge-pane-head">
                            <span class="dm-merge-pane-label">{label}</span>
                            <Button
                                variant="ghost"
                                size="sm"
                                icon={FolderOpen}
                                onclick={() => loadInto(key as 'base' | 'ours' | 'theirs')}
                                disabled={busy}
                            >Load file…</Button>
                        </div>
                        {#if key === 'base'}
                            <Textarea bind:value={base} rows={10} mono disabled={busy} />
                        {:else if key === 'ours'}
                            <Textarea bind:value={ours} rows={10} mono disabled={busy} />
                        {:else}
                            <Textarea bind:value={theirs} rows={10} mono disabled={busy} />
                        {/if}
                    </div>
                {/each}
            </div>
            <div class="dm-actions">
                <Button variant="primary" icon={GitMerge} onclick={runMerge} disabled={busy}>
                    {busy ? 'Merging…' : 'Merge'}
                </Button>
            </div>
        </Card>

        {#if merge}
            <Card>
                <div class="dm-merge-result-head">
                    <span class="dm-merge-status {merge.conflicts ? 'is-conflict' : 'is-clean'}">
                        {merge.conflicts ? 'Merged with conflicts' : 'Clean merge'}
                    </span>
                    <div class="dm-merge-result-actions">
                        <Button variant="ghost" size="sm" icon={Copy} onclick={copyMerged}>Copy</Button>
                        <Button variant="ghost" size="sm" icon={Save} onclick={saveMerged}>Save…</Button>
                    </div>
                </div>
                <Textarea value={merge.merged} rows={14} mono readonly />
            </Card>
        {/if}
    {/if}
</ToolPage>

<style>
    .dm-ok {
        font-size: 13px;
        color: var(--color-success);
    }

    /* ── Dual picker (folder/file) ───────────────────────────── */
    .dm-dual-pickers {
        display: grid;
        grid-template-columns: 1fr;
        gap: 12px;
    }
    @media (min-width: 720px) {
        .dm-dual-pickers {
            grid-template-columns: 1fr 1fr;
        }
    }
    .dm-picker-field {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .dm-picker-label {
        font-size: 12px;
        color: var(--color-muted);
    }
    .dm-picker-btn {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        min-height: 40px;
        padding: 0 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-size: 13px;
        text-align: left;
        cursor: pointer;
        transition: border-color var(--dur-micro) var(--ease-out);
        min-width: 0;
    }
    .dm-picker-btn:hover {
        border-color: var(--color-accent);
    }
    .dm-picker-btn :global(.dm-picker-ico) {
        width: 16px;
        height: 16px;
        flex-shrink: 0;
        color: var(--color-muted);
    }
    .dm-picker-path {
        min-width: 0;
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    /* ── Action rows ─────────────────────────────────────────── */
    .dm-actions {
        display: flex;
        justify-content: flex-end;
        gap: 8px;
        align-items: center;
        flex-wrap: wrap;
        margin-top: 12px;
        padding-top: 12px;
        border-top: 1px solid var(--color-border);
    }

    /* ── Stats + filter chips (folder result) ────────────────── */
    .dm-section-head {
        display: flex;
        align-items: center;
        gap: 8px;
        margin-bottom: 12px;
    }
    .dm-step {
        flex: none;
        display: inline-grid;
        width: 18px;
        height: 18px;
        place-items: center;
        border: 1px solid var(--color-border);
        border-radius: 999px;
        color: var(--color-text-secondary);
        font-size: 10px;
        font-weight: 600;
        line-height: 1;
    }
    .dm-section-title {
        margin: 0;
        color: var(--color-text);
        font-size: 13px;
        font-weight: 600;
    }

    .dm-stats-row {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
        align-items: center;
        margin-bottom: 12px;
    }
    .dm-stat {
        font-size: 11.5px;
        padding: 3px 9px;
        border-radius: var(--radius-pill);
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-muted);
    }
    .dm-stat-add { color: var(--color-success); border-color: color-mix(in srgb, var(--color-success) 40%, var(--color-border)); }
    .dm-stat-rem { color: var(--color-error); border-color: color-mix(in srgb, var(--color-error) 40%, var(--color-border)); }
    .dm-stat-mod { color: var(--color-warning); border-color: color-mix(in srgb, var(--color-warning) 40%, var(--color-border)); }
    .dm-filter-row {
        margin-left: auto;
        display: inline-flex;
        gap: 2px;
    }
    .dm-filter-btn {
        font-size: 11.5px;
        padding: 3px 9px;
        border: none;
        background: transparent;
        color: var(--color-muted);
        cursor: pointer;
        border-radius: var(--radius-control);
    }
    .dm-filter-btn:hover { color: var(--color-text); }
    .dm-filter-btn.is-active {
        background: var(--color-accent-soft);
        color: var(--color-text);
    }

    /* ── View toggle (Unified/Split) ─────────────────────────── */
    .dm-view-toggle-row {
        display: flex;
        align-items: center;
        gap: 8px;
        margin-bottom: 8px;
    }
    .dm-view-toggle-row.dm-view-toggle-inline {
        margin: 0 0 0 auto;
    }
    .dm-view-toggle-label {
        font-size: 11.5px;
        color: var(--color-muted);
    }

    /* ── DM-4 sync row ─────────────────────────────────────── */
    .dm-sync-row {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 6px;
        margin-bottom: 8px;
        padding: 6px 8px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
    }
    .dm-sync-label {
        font-size: 11.5px;
        color: var(--color-muted);
    }

    /* ── Entry list (folder diff) ────────────────────────────── */
    .dm-entry-list {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }
    .dm-entry {
        position: relative;
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        border-radius: var(--radius-card);
        overflow: hidden;
    }
    /* Canonical active pattern (binding): the expanded entry gets the accent
       rounded-pill left strip + panel-2 surface, matching list selection
       everywhere else in the app. */
    .dm-entry.is-expanded {
        border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
    }
    .dm-entry.is-expanded::before {
        content: '';
        position: absolute;
        left: 0;
        top: 8px;
        bottom: 8px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        z-index: 1;
    }
    .dm-entry-row {
        display: flex;
        align-items: center;
        gap: 8px;
        width: 100%;
        padding: 8px 12px;
        background: transparent;
        border: none;
        color: var(--color-text);
        font-size: 13px;
        text-align: left;
        cursor: pointer;
        min-width: 0;
    }
    .dm-entry-row:hover { background: var(--color-panel); }
    .dm-entry-main {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
        flex: 1;
    }
    .dm-entry-dot {
        flex-shrink: 0;
        width: 8px;
        height: 8px;
        border-radius: 50%;
    }
    .dm-entry-path {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .dm-entry-meta {
        font-size: 11px;
        color: var(--color-muted);
        flex-shrink: 0;
    }
    .dm-entry-body {
        border-top: 1px solid var(--color-border);
        padding: 8px 12px;
    }
    .dm-entry-note {
        margin: 0;
        font-size: 12px;
        color: var(--color-muted);
    }

    /* ── Unified diff ─────────────────────────────────────────── */
    .dm-uni {
        max-height: 360px;
        overflow: auto;
        margin: 0;
        padding: 8px;
        background: var(--color-panel);
        border-radius: var(--radius-control);
        font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
        font-size: 11.5px;
        line-height: 1.5;
    }
    .dm-uni :global(.diff-add) { color: var(--color-success); }
    .dm-uni :global(.diff-del) { color: var(--color-error); }
    .dm-uni :global(.diff-hunk) { color: var(--color-accent); }
    .dm-uni :global(.diff-ctx) { color: var(--color-muted); }

    /* ── Side-by-side diff (DM-2) ────────────────────────────── */
    .dm-sbs {
        max-height: 480px;
        overflow: auto;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel);
        font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
        font-size: 11.5px;
        line-height: 1.5;
    }
    .dm-sbs-row {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 1px;
        background: var(--color-border);
    }
    .dm-sbs-cell {
        padding: 1px 8px;
        background: var(--color-panel);
        white-space: pre;
        overflow-x: auto;
        min-width: 0;
        color: var(--color-text);
    }
    .dm-sbs-row.dm-sbs-header > .dm-sbs-cell {
        background: var(--color-panel-2);
        color: var(--color-accent);
    }
    .dm-sbs-row.dm-sbs-change .dm-sbs-left {
        background: color-mix(in srgb, var(--color-error) 8%, var(--color-panel));
    }
    .dm-sbs-row.dm-sbs-change .dm-sbs-right {
        background: color-mix(in srgb, var(--color-success) 8%, var(--color-panel));
    }
    /* If only left has content (deletion), grey the right cell so the
       eye doesn't pair a deletion with a blank "added" line. */
    .dm-sbs-row.dm-sbs-change .dm-sbs-left:empty,
    .dm-sbs-row.dm-sbs-change .dm-sbs-right:empty {
        background: var(--color-panel-2);
    }

    /* ── 3-way merge ─────────────────────────────────────────── */
    .dm-merge-blurb {
        margin: 0 0 8px;
        font-size: 12.5px;
        color: var(--color-muted);
    }
    .dm-merge-blurb :global(code) {
        font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
        font-size: 11px;
        background: var(--color-panel-2);
        padding: 1px 4px;
        border-radius: 4px;
    }
    .dm-merge-grid {
        display: grid;
        grid-template-columns: 1fr;
        gap: 12px;
    }
    @media (min-width: 1024px) {
        .dm-merge-grid {
            grid-template-columns: 1fr 1fr 1fr;
        }
    }
    .dm-merge-pane {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .dm-merge-pane-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
    }
    .dm-merge-pane-label {
        font-size: 12px;
        color: var(--color-muted);
    }
    .dm-merge-result-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        margin-bottom: 8px;
    }
    .dm-merge-status {
        font-size: 12px;
    }
    .dm-merge-status.is-clean { color: var(--color-success); }
    .dm-merge-status.is-conflict { color: var(--color-warning); }
    .dm-merge-result-actions {
        display: inline-flex;
        gap: 6px;
    }
</style>
