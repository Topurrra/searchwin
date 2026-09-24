<script lang="ts">
    import { onMount } from 'svelte';
    import { open, save } from '@tauri-apps/plugin-dialog';
    import { Archive, Eye, EyeOff, FileArchive, FolderOpen, FolderOutput, Lock, PackageOpen, Plus, Trash2 } from '@lucide/svelte';
    import DropZone from '$lib/DropZone.svelte';
    import ToolCancelButton from '$lib/components/ToolCancelButton.svelte';
    import { Button, ToolPage, ToolPanel } from '$lib/ui';
    import {
        addArchiveCreatePaths,
        archiveCreateCancelling,
        archiveCreateError,
        archiveCreateFormat,
        archiveCreateLevel,
        archiveCreateOutputPath,
        archiveCreatePaths,
        archiveCreateProcessing,
        archiveCreateProgress,
        archiveCreateResult,
        archiveExtractCancelling,
        archiveExtractError,
        archiveExtractInfo,
        archiveExtractOutputDir,
        archiveExtractOverwrite,
        archiveExtractPath,
        archiveExtractProcessing,
        archiveExtractProgress,
        archiveExtractResult,
        archiveTab,
        cancelArchiveOperation,
        clearArchiveCreate,
        clearArchiveExtract,
        initArchiveUtility,
        inspectArchive,
        removeArchiveCreatePath,
        runArchiveCreate,
        runArchiveExtract,
        type ArchiveFormat,
    } from '$lib/stores/archiveUtility';

    const formats: Array<{ id: ArchiveFormat; outcome: string; name: string; description: string; supportsPassword: boolean }> = [
        { id: 'zip', outcome: 'Works everywhere', name: 'ZIP', description: 'The safe default for sharing with anyone.', supportsPassword: true },
        { id: '7z', outcome: 'Smallest archive', name: '7Z', description: 'Best general compression for folders and documents.', supportsPassword: true },
        { id: 'tar.zst', outcome: 'Fast large backup', name: 'TAR.ZST', description: 'Modern compression for large folders and backups.', supportsPassword: false },
        { id: 'tar.gz', outcome: 'Unix / server share', name: 'TAR.GZ', description: 'A familiar choice for Linux and server workflows.', supportsPassword: false },
        { id: 'tar.xz', outcome: 'Advanced maximum', name: 'TAR.XZ', description: 'Smaller output, with slower creation and extraction.', supportsPassword: false },
    ];
    const archiveExtensions = ['zip', '7z', 'tar', 'gz', 'tgz', 'zst', 'xz', 'txz'];

    let showCreatePassword = $state(false);
    let createPassword = $state('');
    let showExtractPassword = $state(false);
    let extractPassword = $state('');

    const activeFormat = $derived(formats.find((format) => format.id === $archiveCreateFormat) ?? formats[0]);
    const busy = $derived($archiveCreateProcessing || $archiveExtractProcessing);
    const extractionPasswordSupported = $derived($archiveExtractPath.toLowerCase().endsWith('.zip') || $archiveExtractPath.toLowerCase().endsWith('.7z'));
    const extractionPasswordRequired = $derived(Boolean($archiveExtractInfo?.has_encryption));
    const createCanRun = $derived(Boolean($archiveCreatePaths.length && $archiveCreateOutputPath && !busy && (!activeFormat.supportsPassword || !createPassword || createPassword.trim())));
    const extractCanRun = $derived(Boolean($archiveExtractPath && $archiveExtractOutputDir && !busy && (!extractionPasswordRequired || extractPassword.trim())));

    onMount(() => void initArchiveUtility());

    function fileName(path: string): string {
        return path.split(/[\\/]/).pop() ?? path;
    }

    function formatBytes(bytes: number): string {
        if (!bytes) return '0 B';
        const units = ['B', 'KB', 'MB', 'GB', 'TB'];
        const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
        const value = bytes / (1024 ** exponent);
        return `${value >= 10 || exponent === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[exponent]}`;
    }

    function defaultArchiveName(): string {
        const first = $archiveCreatePaths[0];
        const name = first ? fileName(first).replace(/\.[^.]+$/, '') : 'archive';
        return `${name || 'archive'}.${$archiveCreateFormat}`;
    }

    async function pickFiles(): Promise<void> {
        const selected = await open({ title: 'Choose files to archive', directory: false, multiple: true });
        if (Array.isArray(selected)) addArchiveCreatePaths(selected);
        else if (typeof selected === 'string') addArchiveCreatePaths([selected]);
    }

    async function pickFolders(): Promise<void> {
        const selected = await open({ title: 'Choose folders to archive', directory: true, multiple: true });
        if (Array.isArray(selected)) addArchiveCreatePaths(selected);
        else if (typeof selected === 'string') addArchiveCreatePaths([selected]);
    }

    function chooseFormat(format: ArchiveFormat): void {
        if (busy) return;
        archiveCreateFormat.set(format);
        archiveCreateOutputPath.set('');
        if (!formats.find((item) => item.id === format)?.supportsPassword) createPassword = '';
    }

    async function pickArchiveOutput(): Promise<void> {
        if (!$archiveCreatePaths.length) return;
        const extension = $archiveCreateFormat.split('.').pop() ?? 'zip';
        const selected = await save({
            title: 'Save archive as',
            defaultPath: defaultArchiveName(),
            filters: [{ name: activeFormat.name, extensions: [extension] }],
        });
        if (selected) archiveCreateOutputPath.set(selected);
    }

    async function pickArchive(): Promise<void> {
        const selected = await open({
            title: 'Choose an archive',
            directory: false,
            multiple: false,
            filters: [{ name: 'Archives', extensions: archiveExtensions }],
        });
        if (typeof selected === 'string') {
            extractPassword = '';
            await inspectArchive(selected);
        }
    }

    async function pickExtractFolder(): Promise<void> {
        const selected = await open({ title: 'Choose where to extract', directory: true, multiple: false });
        if (typeof selected === 'string') archiveExtractOutputDir.set(selected);
    }

    function handleDrop(paths: string[]): void {
        if ($archiveTab === 'extract') {
            const [path] = paths;
            if (path) {
                extractPassword = '';
                void inspectArchive(path);
            }
            return;
        }
        addArchiveCreatePaths(paths);
    }

    function progressPercent(progress: { files_done: number; files_total: number } | null): number | null {
        if (!progress?.files_total) return null;
        return Math.min(100, Math.round((progress.files_done / progress.files_total) * 100));
    }
</script>

<DropZone onFiles={handleDrop}>
    <ToolPage icon={Archive} iconTint="#64748b" title="Archive Utility" description="Create compact local archives or inspect and extract one safely. Nothing leaves your device." width="wide" fill={false}>
        {#snippet actions()}
            <ToolCancelButton running={$archiveCreateProcessing || $archiveExtractProcessing} cancelling={$archiveCreateCancelling || $archiveExtractCancelling} onCancel={cancelArchiveOperation} label="Cancel" />
        {/snippet}

        <div class="tabs" role="tablist" aria-label="Archive utility mode">
            <button type="button" role="tab" aria-selected={$archiveTab === 'create'} class:active={$archiveTab === 'create'} onclick={() => archiveTab.set('create')} disabled={busy}><Archive size={16} /> Create archive</button>
            <button type="button" role="tab" aria-selected={$archiveTab === 'extract'} class:active={$archiveTab === 'extract'} onclick={() => archiveTab.set('extract')} disabled={busy}><PackageOpen size={16} /> Extract archive</button>
        </div>

        {#if $archiveTab === 'create'}
            <div class="layout">
                <ToolPanel as="section" padding="md">
                    <div class="head">
                        <div><span class="eyebrow">1. Add files or folders</span><p>Select anything you want to keep together.</p></div>
                        <div class="actions"><Button variant="secondary" size="sm" icon={Plus} onclick={() => void pickFiles()} disabled={busy}>Files</Button><Button variant="secondary" size="sm" icon={FolderOpen} onclick={() => void pickFolders()} disabled={busy}>Folders</Button>{#if $archiveCreatePaths.length}<Button variant="ghost" size="sm" icon={Trash2} onclick={clearArchiveCreate} disabled={busy}>Clear</Button>{/if}</div>
                    </div>
                    {#if $archiveCreatePaths.length}
                        <div class="paths" aria-label="Selected archive items">
                            {#each $archiveCreatePaths as path (path)}
                                <div><FileArchive size={15} /><span title={path}>{fileName(path)}</span><button type="button" aria-label={`Remove ${fileName(path)}`} onclick={() => removeArchiveCreatePath(path)} disabled={busy}>×</button></div>
                            {/each}
                        </div>
                    {:else}
                        <button class="drop" type="button" onclick={() => void pickFiles()} disabled={busy}><Plus size={20} /> Drop files or folders here, or choose files</button>
                    {/if}
                </ToolPanel>

                <ToolPanel as="section" padding="md">
                    <span class="eyebrow">2. Choose the outcome</span>
                    <div class="formats" role="radiogroup" aria-label="Archive format">
                        {#each formats as format}
                            <button type="button" role="radio" aria-checked={$archiveCreateFormat === format.id} class:active={$archiveCreateFormat === format.id} onclick={() => chooseFormat(format.id)} disabled={busy}><strong>{format.outcome}</strong><span>{format.name}</span><small>{format.description}</small></button>
                        {/each}
                    </div>
                    <div class="options">
                        <label class="level"><span>Compression: {$archiveCreateLevel <= 3 ? 'Fast' : $archiveCreateLevel >= 8 ? 'Maximum' : 'Balanced'}</span><input type="range" min="0" max="9" bind:value={$archiveCreateLevel} disabled={busy} aria-label="Compression level" /></label>
                        {#if activeFormat.supportsPassword}
                            <label class="password"><span><Lock size={14} /> Optional password</span><div><input type={showCreatePassword ? 'text' : 'password'} bind:value={createPassword} autocomplete="new-password" placeholder="Leave empty for no password" disabled={busy} /><button type="button" onclick={() => showCreatePassword = !showCreatePassword} aria-label={showCreatePassword ? 'Hide password' : 'Show password'} disabled={busy}>{#if showCreatePassword}<EyeOff size={15} />{:else}<Eye size={15} />{/if}</button></div></label>
                        {/if}
                    </div>
                </ToolPanel>

                <ToolPanel as="section" padding="md">
                    <div class="head"><div><span class="eyebrow">3. Save destination</span><p class:placeholder={!$archiveCreateOutputPath} class="destination" title={$archiveCreateOutputPath || undefined}>{$archiveCreateOutputPath || 'Choose where to save the archive.'}</p></div><Button variant="secondary" size="sm" icon={FolderOutput} onclick={() => void pickArchiveOutput()} disabled={!$archiveCreatePaths.length || busy}>Choose location</Button></div>
                </ToolPanel>

                {#if $archiveCreateError}<div class="error" role="alert">{$archiveCreateError}</div>{/if}
                {#if $archiveCreateResult}<div class="success" role="status">Created {$archiveCreateResult.total_files} files, {formatBytes($archiveCreateResult.archive_size)} at {$archiveCreateResult.output_path}</div>{/if}
                {#if $archiveCreateProcessing}<div class="progress" role="status"><span>Creating archive{#if $archiveCreateProgress?.current_file}: {fileName($archiveCreateProgress.current_file)}{/if}</span><div><i style={`width: ${progressPercent($archiveCreateProgress) ?? 12}%`}></i></div></div>{/if}

                <div class="run"><span>ZIP and 7Z can be password-protected. TAR formats stay unencrypted.</span><Button variant="primary" icon={Archive} loading={$archiveCreateProcessing} onclick={() => void runArchiveCreate(createPassword.trim() || null)} disabled={!createCanRun}>{$archiveCreateProcessing ? 'Creating…' : `Create ${activeFormat.name}`}</Button></div>
            </div>
        {:else}
            <div class="layout">
                <ToolPanel as="section" padding="md">
                    <div class="head"><div><span class="eyebrow">1. Choose an archive</span><p class:placeholder={!$archiveExtractPath} class="destination" title={$archiveExtractPath || undefined}>{$archiveExtractPath || 'ZIP, 7Z, TAR.ZST, TAR.GZ, or TAR.XZ'}</p></div><div class="actions"><Button variant="secondary" size="sm" icon={FolderOpen} onclick={() => void pickArchive()} disabled={busy}>Choose archive</Button>{#if $archiveExtractPath}<Button variant="ghost" size="sm" icon={Eye} onclick={() => void inspectArchive($archiveExtractPath, extractPassword.trim() || null)} disabled={busy}>Inspect</Button><Button variant="ghost" size="sm" icon={Trash2} onclick={clearArchiveExtract} disabled={busy}>Clear</Button>{/if}</div></div>
                    {#if $archiveExtractInfo}
                        <div class="inspect"><div><strong>{$archiveExtractInfo.format.toUpperCase()}</strong><span>{$archiveExtractInfo.entries.length} entries · {formatBytes($archiveExtractInfo.total_size)}</span></div>{#if $archiveExtractInfo.has_encryption}<span class="encrypted"><Lock size={14} /> Password-protected</span>{/if}<div class="entries">{#each $archiveExtractInfo.entries.slice(0, 5) as entry (entry.name)}<span title={entry.name}>{entry.is_dir ? 'Folder' : formatBytes(entry.size)} · {entry.name}</span>{/each}{#if $archiveExtractInfo.entries.length > 5}<span>… and {$archiveExtractInfo.entries.length - 5} more</span>{/if}</div></div>
                    {/if}
                </ToolPanel>

                <ToolPanel as="section" padding="md">
                    <div class="head"><div><span class="eyebrow">2. Extract destination</span><p class:placeholder={!$archiveExtractOutputDir} class="destination" title={$archiveExtractOutputDir || undefined}>{$archiveExtractOutputDir || 'Choose a folder for the extracted files.'}</p></div><Button variant="secondary" size="sm" icon={FolderOutput} onclick={() => void pickExtractFolder()} disabled={!$archiveExtractPath || busy}>Choose folder</Button></div>
                    <div class="options"><label class="check"><input type="checkbox" bind:checked={$archiveExtractOverwrite} disabled={busy} /> Overwrite existing files</label>{#if extractionPasswordSupported}<label class="password"><span><Lock size={14} /> {extractionPasswordRequired ? 'Password required' : 'Password (if needed)'}</span><div><input type={showExtractPassword ? 'text' : 'password'} bind:value={extractPassword} autocomplete="current-password" placeholder={extractionPasswordRequired ? 'Enter archive password' : 'Leave empty if unprotected'} disabled={busy} /><button type="button" onclick={() => showExtractPassword = !showExtractPassword} aria-label={showExtractPassword ? 'Hide password' : 'Show password'} disabled={busy}>{#if showExtractPassword}<EyeOff size={15} />{:else}<Eye size={15} />{/if}</button></div></label>{/if}</div>
                </ToolPanel>

                {#if $archiveExtractError}<div class="error" role="alert">{$archiveExtractError}</div>{/if}
                {#if $archiveExtractResult}<div class="success" role="status">Extracted {$archiveExtractResult.total_files} files to {$archiveExtractResult.output_dir}</div>{/if}
                {#if $archiveExtractProcessing}<div class="progress" role="status"><span>Extracting{#if $archiveExtractProgress?.current_file}: {fileName($archiveExtractProgress.current_file)}{/if}</span><div><i style={`width: ${progressPercent($archiveExtractProgress) ?? 12}%`}></i></div></div>{/if}

                <div class="run"><span>Contents are inspected before extraction. Unsafe archive paths are rejected.</span><Button variant="primary" icon={PackageOpen} loading={$archiveExtractProcessing} onclick={() => void runArchiveExtract(extractPassword || null)} disabled={!extractCanRun}>{$archiveExtractProcessing ? 'Extracting…' : 'Extract archive'}</Button></div>
            </div>
        {/if}
    </ToolPage>
</DropZone>

<style>
    .tabs { display: flex; gap: 6px; margin-bottom: 16px; }
    .tabs button { display: inline-flex; align-items: center; gap: 7px; min-height: 34px; padding: 0 11px; border: 1px solid var(--color-border); border-radius: var(--radius-control); background: transparent; color: var(--color-text-secondary); font-size: 13px; }
    .tabs button.active { border-color: var(--color-accent); color: var(--color-text); }
    .tabs button:focus-visible, .formats button:focus-visible, .drop:focus-visible, .paths button:focus-visible, .password button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
    .layout { display: grid; gap: 12px; }
    .head { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
    .head p { margin: 4px 0 0; color: var(--color-text-secondary); font-size: 13px; }
    .actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 7px; }
    .eyebrow { display: block; color: var(--color-text-secondary); font-size: 11px; font-weight: 600; letter-spacing: .06em; text-transform: uppercase; }
    .paths { display: grid; gap: 4px; margin-top: 14px; }
    .paths > div { display: flex; align-items: center; gap: 8px; min-width: 0; padding: 7px 0; border-top: 1px solid var(--color-border); color: var(--color-text-secondary); font-size: 13px; }
    .paths > div:first-child { border-top: 0; }
    .paths :global(svg), .drop :global(svg) { flex: 0 0 auto; color: var(--color-accent); }
    .paths span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .paths button { margin-left: auto; border: 0; background: transparent; color: var(--color-text-secondary); font-size: 18px; line-height: 1; }
    .drop { display: flex; align-items: center; justify-content: center; gap: 8px; width: 100%; min-height: 82px; margin-top: 14px; border: 1px dashed var(--color-border-strong); border-radius: var(--radius-control); background: var(--color-panel-2); color: var(--color-text-secondary); font-size: 13px; }
    .formats { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin-top: 12px; }
    .formats button { display: grid; gap: 3px; min-height: 88px; padding: 12px; border: 1px solid var(--color-border); border-radius: var(--radius-control); background: var(--color-panel-2); color: var(--color-text-secondary); text-align: left; }
    .formats button.active { border-color: var(--color-accent); color: var(--color-text); }
    .formats strong { font-size: 13px; }
    .formats span { color: var(--color-accent); font-size: 11px; font-weight: 700; letter-spacing: .04em; }
    .formats small { font-size: 12px; line-height: 1.35; }
    .options { display: flex; flex-wrap: wrap; gap: 14px; margin-top: 16px; }
    .level, .password { display: grid; flex: 1 1 240px; gap: 7px; color: var(--color-text-secondary); font-size: 12px; }
    .level input, .check input { accent-color: var(--color-accent); }
    .password > span { display: inline-flex; align-items: center; gap: 6px; }
    .password > div { display: flex; overflow: hidden; border: 1px solid var(--color-border); border-radius: var(--radius-control); background: var(--color-panel-2); }
    .password input { min-width: 0; flex: 1; height: 34px; padding: 0 9px; border: 0; outline: 0; background: transparent; color: var(--color-text); font: inherit; }
    .password button { display: grid; width: 34px; place-items: center; border: 0; background: transparent; color: var(--color-text-secondary); }
    .destination { max-width: 650px; overflow: hidden; color: var(--color-text); font-family: var(--font-mono, ui-monospace, monospace); font-size: 12px; text-overflow: ellipsis; white-space: nowrap; }
    .destination.placeholder { color: var(--color-text-secondary); font-family: inherit; }
    .error, .success { padding: 10px 12px; border-left: 2px solid; font-size: 13px; line-height: 1.45; white-space: pre-wrap; }
    .error { border-color: var(--color-error); color: var(--color-error); }
    .success { border-color: var(--color-success, var(--color-accent)); color: var(--color-text-secondary); }
    .progress { display: grid; gap: 8px; padding: 10px 12px; border-left: 2px solid var(--color-accent); color: var(--color-text-secondary); font-size: 13px; }
    .progress > div { height: 3px; overflow: hidden; background: var(--color-border); }
    .progress i { display: block; height: 100%; min-width: 12%; background: var(--color-accent); transition: width 160ms ease; }
    .run { display: flex; align-items: center; justify-content: space-between; gap: 14px; color: var(--color-text-secondary); font-size: 12px; }
    .check { display: inline-flex; align-items: center; gap: 7px; color: var(--color-text-secondary); font-size: 13px; }
    .inspect { display: grid; gap: 10px; margin-top: 14px; padding-top: 14px; border-top: 1px solid var(--color-border); }
    .inspect > div:first-child { display: flex; align-items: baseline; gap: 9px; }
    .inspect > div:first-child span, .encrypted { color: var(--color-text-secondary); font-size: 12px; }
    .encrypted { display: inline-flex; align-items: center; gap: 6px; }
    .entries { display: grid; gap: 4px; color: var(--color-text-secondary); font-family: var(--font-mono, ui-monospace, monospace); font-size: 11px; }
    .entries span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    @media (max-width: 640px) { .formats { grid-template-columns: 1fr; } .head, .run { align-items: stretch; flex-direction: column; } .actions { justify-content: flex-start; } }
    @media (prefers-reduced-motion: reduce) { .progress i { transition: none; } }
</style>
