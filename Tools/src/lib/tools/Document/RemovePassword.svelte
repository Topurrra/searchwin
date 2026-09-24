<script lang="ts">
    /*
      Remove Password — strip the "restrict editing" lock from Word .docx files.

      DOCX "edit restriction" is non-cryptographic: it's a `documentProtection`
      XML flag inside the OOXML container. We strip it without a password and
      write an unlocked copy alongside the original (or in a chosen folder).

      Scope note (2026-06): PDF unlocking has moved to KeepItLocal Privacy, so
      this tool is DOCX-only. Files that are actually encrypted (a CFBF
      "open password" container masquerading as a .docx) get a clear error
      pointing the user at Word's "Save without password" workflow.
    */
    import { invoke } from '@tauri-apps/api/core';
    import { open as openDialog } from '@tauri-apps/plugin-dialog';
    import {
        Lock,
        Plus,
        Trash2,
        Play,
        FileText,
        ShieldCheck,
        AlertTriangle,
        FolderOpen,
    } from '@lucide/svelte';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import { recordActivity } from '$lib/stores/activityLog';
    import { reportBusy } from '$lib/stores/globalBusy';
    import LoadingState from '$lib/components/LoadingState.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import { ToolPage } from '$lib/ui';
    import {
        removePasswordFiles,
        removePasswordOutputDir,
        removePasswordWorking,
        type RemovePasswordFileKind,
        type RemovePasswordResult,
        type RemovePasswordFile,
    } from '$lib/stores/removePassword';

    type FileKind = RemovePasswordFileKind;
    type SourceFile = RemovePasswordFile;

    // Persistent user-produced state lives in $lib/stores/removePassword so
    // the picked files, their unlock results, the chosen output dir, and the
    // in-flight flag survive leaving and returning to the tool (the
    // workspace remounts this panel fresh on navigation).

    function detectKind(path: string): FileKind {
        const ext = path.split('.').pop()?.toLowerCase();
        if (ext === 'docx') return 'docx';
        return 'unsupported';
    }

    function basename(path: string): string {
        return path.split(/[\\/]/).pop() ?? path;
    }

    async function pickFiles() {
        try {
            const result = await openDialog({
                multiple: true,
                directory: false,
                filters: [{ name: 'Word documents', extensions: ['docx'] }],
            });
            if (!result) return;
            const picked = Array.isArray(result) ? result : [result];
            const current = $removePasswordFiles;
            const additions: SourceFile[] = picked
                .filter((p) => !current.some((f) => f.path === p))
                .map((path) => ({ path, name: basename(path), kind: detectKind(path) }));
            removePasswordFiles.set([...current, ...additions]);
        } catch (error) {
            errorToast("Couldn't open the file picker", error, {
                hint: 'Try again — Windows occasionally refuses dialog focus during heavy activity.',
            });
        }
    }

    async function pickOutputDir() {
        try {
            const result = await openDialog({ multiple: false, directory: true });
            if (typeof result === 'string') {
                removePasswordOutputDir.set(result);
            }
        } catch (error) {
            errorToast("Couldn't open the folder picker", error, {
                hint: 'Try again — Windows occasionally refuses dialog focus during heavy activity.',
            });
        }
    }

    function clearAll() {
        if ($removePasswordWorking) return;
        removePasswordFiles.set([]);
    }

    function removeFile(path: string) {
        if ($removePasswordWorking) return;
        removePasswordFiles.update((list) => list.filter((f) => f.path !== path));
    }

    let docxFiles = $derived($removePasswordFiles.filter((f) => f.kind === 'docx'));
    let unsupportedFiles = $derived($removePasswordFiles.filter((f) => f.kind === 'unsupported'));

    async function run() {
        if ($removePasswordWorking) return;
        if ($removePasswordFiles.length === 0) {
            toast('Add a .docx file first.', 'info');
            return;
        }
        if (docxFiles.length === 0) {
            toast('Only .docx files are supported.', 'error');
            return;
        }

        removePasswordWorking.set(true);
        const stopBusy = reportBusy('doc-password', 'Removing edit restrictions');
        try {
            await runDocxUnlock();

            const successCount = $removePasswordFiles.filter((f) => f.result?.success).length;
            const failCount = $removePasswordFiles.filter((f) => f.result?.success === false).length;
            if (successCount > 0 && failCount === 0) {
                toast(`Unlocked ${successCount} file${successCount === 1 ? '' : 's'}.`, 'success');
                void recordActivity({
                    toolId: 'doc-password',
                    summary: `Removed edit restrictions from ${successCount} Word doc${successCount === 1 ? '' : 's'}`,
                    outcome: 'success',
                });
            } else if (successCount > 0 && failCount > 0) {
                toast(`Unlocked ${successCount}, ${failCount} failed (see rows for details).`, 'info', 5000);
                void recordActivity({
                    toolId: 'doc-password',
                    summary: `Remove Password finished with errors`,
                    details: `${successCount} unlocked · ${failCount} failed`,
                    outcome: 'success',
                });
            } else if (failCount > 0) {
                toast(`All ${failCount} file${failCount === 1 ? '' : 's'} failed — see rows for details.`, 'error', 5000);
                void recordActivity({
                    toolId: 'doc-password',
                    summary: `Remove Password failed`,
                    details: `All ${failCount} file${failCount === 1 ? '' : 's'} failed`,
                    outcome: 'failed',
                });
            }
        } catch (error) {
            errorToast("Couldn't remove the edit restrictions", error, {
                hint: "Check the file isn't open in Word and is a real .docx (not a renamed .doc).",
                durationMs: 5500,
            });
            void recordActivity({
                toolId: 'doc-password',
                summary: `Remove Password crashed`,
                details: String(error),
                outcome: 'failed',
            });
        } finally {
            removePasswordWorking.set(false);
            stopBusy();
        }
    }

    async function runDocxUnlock() {
        try {
            const results = await invoke<
                Array<{
                    sourcePath: string;
                    outputPath?: string;
                    success: boolean;
                    error?: string;
                    protectionRemoved?: boolean;
                }>
            >('remove_docx_password', {
                options: {
                    paths: docxFiles.map((f) => f.path),
                    outputDir: $removePasswordOutputDir,
                    suffix: '_unlocked',
                },
            });
            applyResults(results);
        } catch (error) {
            for (const g of docxFiles) {
                setResult(g.path, { success: false, error: String(error) });
            }
        }
    }

    function applyResults(
        results: Array<{
            sourcePath: string;
            outputPath?: string;
            success: boolean;
            error?: string;
            protectionRemoved?: boolean;
        }>,
    ) {
        for (const r of results) {
            setResult(r.sourcePath, {
                success: r.success,
                outputPath: r.outputPath,
                error: r.error,
                protectionRemoved: r.protectionRemoved,
            });
        }
    }

    function setResult(path: string, partial: RemovePasswordResult) {
        removePasswordFiles.update((list) =>
            list.map((f) => (f.path === path ? { ...f, result: partial } : f)),
        );
    }
</script>

<ToolPage
    icon={Lock}
    iconTint="#fb7185"
    title="Remove Word edit restrictions"
    description="Strip the “restrict editing” lock from Word .docx files — no password needed, because Word's edit-restriction is just an XML flag. Originals are preserved; unlocked copies land alongside or in your chosen folder."
    width="wide"
    fill={false}
>
    <!-- Toolbar -->
    <div class="flex flex-wrap items-center gap-3 rounded-2xl border border-border bg-panel p-3 md:p-4">
        <button
            type="button"
            onclick={pickFiles}
            disabled={$removePasswordWorking}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
        >
            <Plus class="h-4 w-4" />
            Add files
        </button>

        <button
            type="button"
            onclick={pickOutputDir}
            disabled={$removePasswordWorking}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
            title={$removePasswordOutputDir ?? 'Save alongside originals'}
        >
            <FolderOpen class="h-4 w-4" />
            {$removePasswordOutputDir ? `Output: ${basename($removePasswordOutputDir)}` : 'Output: alongside originals'}
        </button>

        {#if $removePasswordFiles.length > 0}
            <button
                type="button"
                onclick={clearAll}
                disabled={$removePasswordWorking}
                class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-error-strong disabled:opacity-50"
            >
                <Trash2 class="h-4 w-4" />
                Clear ({$removePasswordFiles.length})
            </button>
        {/if}

        <div class="ml-auto flex items-center gap-2">
            <button
                type="button"
                onclick={run}
                disabled={$removePasswordWorking || $removePasswordFiles.length === 0}
                class="inline-flex h-10 items-center gap-1.5 rounded-xl bg-accent px-4 text-sm font-medium text-accent-contrast hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed"
            >
                {#if $removePasswordWorking}
                    <LoadingState variant="inline" label="Removing…" />
                {:else}
                    <Play class="h-4 w-4" />
                    Remove restrictions {$removePasswordFiles.length > 0 ? $removePasswordFiles.length : ''}
                {/if}
            </button>
        </div>
    </div>

    <!-- File list -->
    <div class="rounded-2xl border border-border bg-panel p-4 md:p-5 relative">
        {#if $removePasswordFiles.length === 0}
            <EmptyState
                icon={FileText}
                title="No files yet"
                description="Add Word .docx files with edit restrictions to strip them. No password is needed — Word's “restrict editing” is a non-cryptographic XML flag. (PDF unlocking now lives in KeepItLocal Privacy.)"
            >
                {#snippet actions()}
                    <button
                        type="button"
                        onclick={pickFiles}
                        class="inline-flex items-center gap-1.5 rounded-lg border border-accent/40 bg-accent/10 px-3 py-1.5 text-sm text-accent hover:bg-accent/20"
                    >
                        <Plus class="h-4 w-4" />
                        Add files
                    </button>
                {/snippet}
            </EmptyState>
        {:else}
            <div class="space-y-2">
                {#each $removePasswordFiles as file (file.path)}
                    <article
                        class="rounded-xl border p-3 transition-colors
                               {file.result?.success
                            ? 'border-success-strong bg-success-soft'
                            : file.result?.success === false
                              ? 'border-error-strong bg-error-soft'
                              : 'border-border bg-panel-2'}"
                    >
                        <div class="flex items-start gap-3">
                            <!-- Type pill -->
                            <span
                                class="inline-flex h-6 shrink-0 items-center rounded-md px-2 text-[10px] font-semibold uppercase tracking-wider mt-0.5
                                       {file.kind === 'docx'
                                    ? 'bg-info-soft text-info border border-info-strong'
                                    : 'bg-panel border border-border text-muted'}"
                            >
                                {file.kind === 'unsupported' ? '?' : file.kind}
                            </span>

                            <div class="min-w-0 flex-1">
                                <div class="text-sm font-medium text-text truncate" title={file.path}>
                                    {file.name}
                                </div>
                                <div class="text-[11px] text-muted truncate" title={file.path}>
                                    {file.path}
                                </div>

                                <!-- Type-specific row body -->
                                {#if file.kind === 'docx'}
                                    <div class="mt-2 inline-flex items-center gap-1.5 text-[11px] text-muted">
                                        <ShieldCheck class="h-3 w-3 text-success" />
                                        Edit-restriction will be removed (no password needed for .docx)
                                    </div>
                                {:else}
                                    <div class="mt-2 inline-flex items-center gap-1.5 text-[11px] text-error">
                                        <AlertTriangle class="h-3 w-3" />
                                        Unsupported format. Only .docx is supported.
                                    </div>
                                {/if}

                                <!-- Result row -->
                                {#if file.result}
                                    <div
                                        class="mt-2 text-[11px] {file.result.success
                                            ? 'text-success'
                                            : 'text-error'}"
                                    >
                                        {#if file.result.success}
                                            {#if file.result.protectionRemoved === false}
                                                ⓘ No protection found — copy saved at {file.result.outputPath}
                                            {:else}
                                                ✓ Unlocked → {file.result.outputPath}
                                            {/if}
                                        {:else}
                                            ✗ {file.result.error}
                                        {/if}
                                    </div>
                                {/if}
                            </div>

                            <button
                                type="button"
                                onclick={() => removeFile(file.path)}
                                disabled={$removePasswordWorking}
                                class="rounded-lg border border-border bg-panel p-2 text-muted hover:text-error hover:border-error-strong disabled:opacity-50"
                                title="Remove from list"
                                aria-label="Remove"
                            >
                                <Trash2 class="h-3.5 w-3.5" />
                            </button>
                        </div>
                    </article>
                {/each}
            </div>

            {#if unsupportedFiles.length > 0}
                <div class="mt-3 rounded-lg border border-warning-strong bg-warning-soft p-3 text-xs text-warning">
                    {unsupportedFiles.length} file{unsupportedFiles.length === 1 ? '' : 's'}
                    in the list {unsupportedFiles.length === 1 ? 'is' : 'are'} not supported and will be skipped.
                </div>
            {/if}
        {/if}
    </div>
</ToolPage>
