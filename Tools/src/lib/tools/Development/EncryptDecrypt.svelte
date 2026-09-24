<script lang="ts">
    import { onMount } from 'svelte';
    import { open } from '@tauri-apps/plugin-dialog';
    import { fromStore } from 'svelte/store';
    import { Copy, Eye, EyeOff, FileKey2, FolderOpen, KeyRound, LockKeyhole, Play, X } from '@lucide/svelte';
    import { ToolPage } from '$lib/ui';
    import {
        encryptText,
        decryptText,
        inspectFile,
        cancelCryptoOperation,
        passwordStrength,
    } from '$lib/stores/crypto';
    import {
        cryptoTab,
        cryptoMode,
        cryptoBusy,
        cryptoFiles,
        cryptoResults,
        cryptoCurrentOpId,
        cryptoCancelRequested,
        runCryptoFiles,
    } from '$lib/stores/cryptoState';
    import { subscribeToToolLaunchTarget } from '$lib/stores/toolLaunchTarget';

    // Module-owned runtime state remains live after CategoryWorkspace unmounts.
    const tabStore = fromStore(cryptoTab);
    const modeStore = fromStore(cryptoMode);
    const busyStore = fromStore(cryptoBusy);
    const filesStore = fromStore(cryptoFiles);
    const resultsStore = fromStore(cryptoResults);
    const operationStore = fromStore(cryptoCurrentOpId);
    let tab = $derived(tabStore.current);
    let mode = $derived(modeStore.current);
    let busy = $derived(busyStore.current);
    let files = $derived(filesStore.current);
    let results = $derived(resultsStore.current);
    let currentOpId = $derived(operationStore.current);

    let message = $state<{ kind: 'ok' | 'err'; text: string } | null>(null);

    // Secrets remain local and are captured only by the active operation.
    let password = $state('');
    let password2 = $state('');
    let showPassword = $state(false);
    let useKeyfile = $state(false);
    let keyfilePath = $state('');
    let textInput = $state('');
    let textOutput = $state('');

    onMount(() =>
        subscribeToToolLaunchTarget('encrypt-decrypt', ({ targetFile }) => {
            cryptoTab.set('files');
            cryptoMode.set('encrypt');
            cryptoFiles.update((current) =>
                current.includes(targetFile) ? current : [...current, targetFile],
            );
        }),
    );

    const strength = $derived(passwordStrength(password));
    const passMismatch = $derived(mode === 'encrypt' && password.length > 0 && password !== password2);
    const hasSecret = $derived(password.length > 0 || (useKeyfile && keyfilePath.length > 0));

    function flash(kind: 'ok' | 'err', text: string) {
        message = { kind, text };
        setTimeout(() => (message = null), 5000);
    }

    function basename(p: string) {
        return p.split(/[\\/]/).pop() ?? p;
    }

    async function pickFiles() {
        if (busy) return;
        const picked = await open({ multiple: true, title: mode === 'encrypt' ? 'Choose files to encrypt' : 'Choose encrypted files to unlock' });
        if (busy || !picked) return;
        const list = Array.isArray(picked) ? picked : [picked];
        cryptoFiles.update((current) => [...new Set([...current, ...list])]);
    }
    function removeFile(path: string) {
        if (busy) return;
        cryptoFiles.update((current) => current.filter((file) => file !== path));
    }
    async function pickKeyfile() {
        if (busy) return;
        const picked = await open({ multiple: false, title: 'Choose a keyfile' });
        if (!busy && picked && !Array.isArray(picked)) keyfilePath = picked;
    }

    const keyfileArg = $derived(useKeyfile && keyfilePath ? keyfilePath : undefined);

    async function runFiles() {
        if (busy || files.length === 0 || !hasSecret || passMismatch) return;
        const runMode = mode;
        const summary = await runCryptoFiles({
            mode: runMode,
            files,
            password,
            keyfilePath: keyfileArg,
        });
        if (!summary) return;
        if (summary.cancelled) flash('err', `Cancelled (${summary.ok} done, ${summary.fail} cancelled/failed)`);
        else if (summary.fail === 0) flash('ok', `${runMode === 'encrypt' ? 'Encrypted' : 'Decrypted'} ${summary.ok} file${summary.ok === 1 ? '' : 's'}`);
        else flash('err', `${summary.ok} ok, ${summary.fail} failed`);
    }

    async function cancelRun() {
        if (!currentOpId) return;
        cryptoCancelRequested.set(true);
        try {
            await cancelCryptoOperation(currentOpId);
        } catch {
            // Best effort: the queue stop request still blocks later files.
        }
    }

    async function runText() {
        if (busy || !hasSecret || passMismatch || !textInput.trim()) return;
        const runMode = mode;
        const runPassword = password;
        const runKeyfile = keyfileArg;
        const input = textInput;
        cryptoBusy.set(true);
        textOutput = '';
        try {
            textOutput =
                runMode === 'encrypt'
                    ? await encryptText({ text: input, password: runPassword, keyfilePath: runKeyfile })
                    : await decryptText({ payload: input, password: runPassword, keyfilePath: runKeyfile });
            flash('ok', runMode === 'encrypt' ? 'Encrypted' : 'Decrypted');
        } catch (error) {
            flash('err', String(error));
        } finally {
            cryptoBusy.set(false);
        }
    }

    async function copyOutput() {
        try {
            await navigator.clipboard.writeText(textOutput);
            flash('ok', 'Copied');
        } catch {
            flash('err', 'Could not copy');
        }
    }

    // When choosing decrypt + files, warn if any needs a keyfile.
    $effect(() => {
        if (mode === 'decrypt' && tab === 'files' && files.length > 0) {
            void (async () => {
                for (const f of files) {
                    try {
                        const info = await inspectFile(f);
                        if (info.valid && info.keyfileRequired && !useKeyfile) {
                            flash('err', `${basename(f)} needs a keyfile — enable "Use a keyfile".`);
                            break;
                        }
                    } catch {
                        /* ignore */
                    }
                }
            })();
        }
    });
</script>

<ToolPage
    icon={LockKeyhole}
    iconTint="#22c55e"
    title="Encrypt / Decrypt"
    description="Password-encrypt files or text with authenticated AES-256-GCM. Everything stays on this device."
    width="wide"
    fill={false}
>
    <div class="crypto-workbench">
        {#if message}
            <div class="crypto-notice" class:is-error={message.kind === 'err'} role="status">{message.text}</div>
        {/if}

        <div class="crypto-command-bar">
            <div class="crypto-segment" role="group" aria-label="Content type">
                <button
                    type="button"
                    class="crypto-segment-button"
                    class:is-active={tab === 'files'}
                    aria-pressed={tab === 'files'}
                    disabled={busy}
                    onclick={() => cryptoTab.set('files')}
                >Files</button>
                <button
                    type="button"
                    class="crypto-segment-button"
                    class:is-active={tab === 'text'}
                    aria-pressed={tab === 'text'}
                    disabled={busy}
                    onclick={() => cryptoTab.set('text')}
                >Text</button>
            </div>
            <div class="crypto-command-spacer"></div>
            <div class="crypto-segment" role="group" aria-label="Operation">
                <button
                    type="button"
                    class="crypto-segment-button"
                    class:is-active={mode === 'encrypt'}
                    aria-pressed={mode === 'encrypt'}
                    disabled={busy}
                    onclick={() => cryptoMode.set('encrypt')}
                >Encrypt</button>
                <button
                    type="button"
                    class="crypto-segment-button"
                    class:is-active={mode === 'decrypt'}
                    aria-pressed={mode === 'decrypt'}
                    disabled={busy}
                    onclick={() => cryptoMode.set('decrypt')}
                >Decrypt</button>
            </div>
        </div>

        <section class="crypto-credentials" aria-label="Credentials">
            <div class="crypto-section-head">
                <div>
                    <span class="crypto-eyebrow">Credentials</span>
                    <p>{mode === 'encrypt' ? 'Choose a password before writing new encrypted copies.' : 'Use the original password and optional keyfile to open copies.'}</p>
                </div>
                <KeyRound size={18} aria-hidden="true" />
            </div>
            <div class="crypto-credential-grid">
                <label class="crypto-field">
                    <span>Password</span>
                    <div class="crypto-password-field">
                        <input type={showPassword ? 'text' : 'password'} bind:value={password} disabled={busy} autocomplete="new-password" />
                        <button type="button" class="crypto-icon-button" onclick={() => (showPassword = !showPassword)} disabled={busy} aria-label={showPassword ? 'Hide password' : 'Show password'}>
                            {#if showPassword}<EyeOff size={16} />{:else}<Eye size={16} />{/if}
                        </button>
                    </div>
                    {#if mode === 'encrypt' && password}
                        <span class="crypto-strength" style={`--strength-color: ${strength.color}`}>
                            <span><i style={`width: ${(strength.score / 4) * 100}%`}></i></span>{strength.label}
                        </span>
                    {/if}
                </label>
                {#if mode === 'encrypt'}
                    <label class="crypto-field">
                        <span>Confirm password</span>
                        <input type={showPassword ? 'text' : 'password'} bind:value={password2} disabled={busy} autocomplete="new-password" class:is-invalid={passMismatch} />
                        {#if passMismatch}<small>Passwords do not match.</small>{/if}
                    </label>
                {/if}
            </div>
            <label class="crypto-keyfile-toggle">
                <input type="checkbox" bind:checked={useKeyfile} disabled={busy} />
                <span>Require a keyfile too <em>Both are needed to decrypt.</em></span>
            </label>
            {#if useKeyfile}
                <div class="crypto-keyfile-row">
                    <span class="crypto-keyfile-path" title={keyfilePath || undefined}>{keyfilePath || 'No keyfile selected'}</span>
                    <button type="button" class="crypto-quiet-button" onclick={pickKeyfile} disabled={busy}><FolderOpen size={15} /> Choose keyfile</button>
                </div>
                <p class="crypto-keyfile-warning">Keep it safe and unchanged. Losing or editing the keyfile makes encrypted data unrecoverable.</p>
            {/if}
        </section>

        {#if tab === 'files'}
            <div class="crypto-file-workspace">
                <section class="crypto-queue">
                    <div class="crypto-section-head">
                        <div>
                            <span class="crypto-eyebrow">Source files</span>
                            <p>{mode === 'encrypt' ? 'Original files stay untouched; encrypted copies are written beside them.' : 'Choose .kenc files to restore beside the encrypted originals.'}</p>
                        </div>
                        <button type="button" class="crypto-quiet-button" onclick={pickFiles} disabled={busy}><FolderOpen size={15} /> Add files</button>
                    </div>
                    {#if files.length === 0}
                        <div class="crypto-empty"><FileKey2 size={22} /><span>{mode === 'decrypt' ? 'Choose encrypted files to unlock.' : 'Choose files to encrypt.'}</span></div>
                    {:else}
                        <div class="crypto-file-list">
                            {#each files as file (file)}
                                <div class="crypto-file-row">
                                    <FileKey2 size={16} aria-hidden="true" />
                                    <span title={file}>{basename(file)}</span>
                                    <button type="button" class="crypto-icon-button is-danger" onclick={() => removeFile(file)} disabled={busy} aria-label={`Remove ${basename(file)}`}>
                                        <X size={15} />
                                    </button>
                                </div>
                            {/each}
                        </div>
                    {/if}
                    <div class="crypto-run-row">
                        <button type="button" class="crypto-primary-button" onclick={runFiles} disabled={busy || files.length === 0 || !hasSecret || passMismatch}>
                            <Play size={16} /> {busy ? 'Working…' : mode === 'encrypt' ? `Encrypt ${files.length} file${files.length === 1 ? '' : 's'}` : `Decrypt ${files.length} file${files.length === 1 ? '' : 's'}`}
                        </button>
                        {#if busy}
                            <button type="button" class="crypto-quiet-button" onclick={cancelRun}>Cancel requested</button>
                        {/if}
                    </div>
                </section>

                <section class="crypto-results">
                    <div class="crypto-section-head">
                        <div>
                            <span class="crypto-eyebrow">Activity</span>
                            <p>{busy ? 'Your current queue is running. You can navigate away and return safely.' : results.length ? 'Completed file operations remain here for review.' : 'Results will appear here after the run.'}</p>
                        </div>
                    </div>
                    {#if results.length > 0}
                        <div class="crypto-result-list">
                            {#each results as result (result.input)}
                                <div class="crypto-result-row" class:is-error={Boolean(result.error)}>
                                    <span>{basename(result.input)}</span>
                                    {#if result.output}<small title={result.output}>→ {result.output}</small>{/if}
                                    {#if result.error}<small>{result.error}</small>{/if}
                                </div>
                            {/each}
                        </div>
                    {:else}
                        <div class="crypto-empty is-quiet"><LockKeyhole size={22} /><span>Nothing has run in this session yet.</span></div>
                    {/if}
                </section>
            </div>
        {:else}
            <div class="crypto-text-workspace">
                <section class="crypto-text-panel">
                    <div class="crypto-section-head"><div><span class="crypto-eyebrow">Input</span><p>{mode === 'encrypt' ? 'Type or paste a message to protect.' : 'Paste encrypted Base64 text to restore.'}</p></div></div>
                    <textarea bind:value={textInput} disabled={busy} spellcheck="false" placeholder={mode === 'encrypt' ? 'Type or paste a secret message…' : 'Paste encrypted Base64 text…'}></textarea>
                    <button type="button" class="crypto-primary-button" onclick={runText} disabled={busy || !hasSecret || passMismatch || !textInput.trim()}><Play size={16} /> {busy ? 'Working…' : mode === 'encrypt' ? 'Encrypt text' : 'Decrypt text'}</button>
                </section>
                <section class="crypto-text-panel">
                    <div class="crypto-section-head"><div><span class="crypto-eyebrow">Output</span><p>{mode === 'encrypt' ? 'Encrypted Base64 output' : 'Recovered text'}</p></div>{#if textOutput}<button type="button" class="crypto-quiet-button" onclick={copyOutput}><Copy size={15} /> Copy</button>{/if}</div>
                    <textarea value={textOutput} readonly spellcheck="false" placeholder="Output appears here after processing."></textarea>
                </section>
            </div>
        {/if}
    </div>
</ToolPage>

<style>
    .crypto-workbench { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
    .crypto-command-bar { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; padding-bottom: 10px; border-bottom: 1px solid var(--color-divider, var(--color-border)); }
    .crypto-command-spacer { flex: 1; }
    .crypto-segment { display: inline-flex; gap: 3px; padding: 3px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); background: var(--color-panel); }
    .crypto-segment-button { position: relative; padding: 6px 14px 6px 17px; border: 0; border-radius: calc(var(--radius-control, 8px) - 2px); background: transparent; color: var(--color-text-secondary); font-size: 12.5px; cursor: pointer; }
    .crypto-segment-button:hover:not(:disabled) { color: var(--color-text); }
    .crypto-segment-button.is-active { background: var(--color-panel-2); color: var(--color-text); }
    .crypto-segment-button.is-active::before { content: ''; position: absolute; left: 5px; top: 7px; bottom: 7px; width: 3px; border-radius: 999px; background: var(--color-accent); }
    .crypto-segment-button:disabled { opacity: 0.55; cursor: default; }
    .crypto-segment-button:focus-visible, .crypto-primary-button:focus-visible, .crypto-quiet-button:focus-visible, .crypto-icon-button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
    .crypto-notice { padding: 9px 12px; border-left: 2px solid var(--color-success); color: var(--color-text-secondary); background: color-mix(in srgb, var(--color-success) 7%, transparent); font-size: 12.5px; }
    .crypto-notice.is-error { border-left-color: var(--color-error); background: color-mix(in srgb, var(--color-error) 7%, transparent); color: var(--color-error); }
    .crypto-credentials, .crypto-queue, .crypto-results, .crypto-text-panel { min-width: 0; border: 1px solid var(--color-border); border-radius: var(--radius-card, 12px); background: var(--color-panel); }
    .crypto-credentials { padding: 16px; }
    .crypto-section-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
    .crypto-section-head :global(svg) { flex: none; color: var(--color-accent); }
    .crypto-section-head p { margin: 3px 0 0; color: var(--color-muted); font-size: 12px; line-height: 1.45; max-width: 62ch; }
    .crypto-eyebrow { color: var(--color-muted); font-size: 10.5px; font-weight: 600; letter-spacing: .05em; text-transform: uppercase; }
    .crypto-credential-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; margin-top: 14px; }
    .crypto-field { display: flex; flex-direction: column; gap: 6px; color: var(--color-text-secondary); font-size: 12px; }
    .crypto-field input, .crypto-text-panel textarea { width: 100%; box-sizing: border-box; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); background: var(--color-panel-2); color: var(--color-text); font: inherit; }
    .crypto-field input { min-height: 36px; padding: 0 10px; }
    .crypto-field input:focus-visible, .crypto-text-panel textarea:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 1px; }
    .crypto-field input.is-invalid { border-color: var(--color-error); }
    .crypto-field small { color: var(--color-error); }
    .crypto-password-field { display: flex; gap: 6px; }
    .crypto-password-field input { min-width: 0; flex: 1; }
    .crypto-icon-button { display: inline-flex; align-items: center; justify-content: center; flex: none; width: 36px; min-height: 36px; padding: 0; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); background: transparent; color: var(--color-muted); cursor: pointer; }
    .crypto-icon-button:hover:not(:disabled) { color: var(--color-text); border-color: color-mix(in srgb, var(--color-text) 24%, var(--color-border)); }
    .crypto-icon-button.is-danger:hover:not(:disabled) { color: var(--color-error); border-color: color-mix(in srgb, var(--color-error) 42%, var(--color-border)); }
    .crypto-icon-button:disabled { opacity: .45; cursor: default; }
    .crypto-strength { display: flex; align-items: center; gap: 8px; color: var(--strength-color); font-size: 11px; }
    .crypto-strength > span { display: block; flex: 1; height: 4px; overflow: hidden; border-radius: 99px; background: var(--color-panel); }
    .crypto-strength i { display: block; height: 100%; border-radius: inherit; background: var(--strength-color); }
    .crypto-keyfile-toggle { display: flex; align-items: flex-start; gap: 8px; margin-top: 14px; color: var(--color-text-secondary); font-size: 12px; }
    .crypto-keyfile-toggle input { margin: 2px 0 0; accent-color: var(--color-accent); }
    .crypto-keyfile-toggle em { margin-left: 4px; color: var(--color-muted); font-style: normal; }
    .crypto-keyfile-row { display: flex; gap: 8px; margin-top: 10px; }
    .crypto-keyfile-path { min-width: 0; flex: 1; overflow: hidden; padding: 9px 10px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); color: var(--color-muted); font-size: 12px; text-overflow: ellipsis; white-space: nowrap; }
    .crypto-keyfile-warning { margin: 7px 0 0; color: var(--color-muted); font-size: 11px; line-height: 1.45; }
    .crypto-quiet-button, .crypto-primary-button { display: inline-flex; align-items: center; justify-content: center; gap: 7px; min-height: 34px; padding: 0 11px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); background: transparent; color: var(--color-text-secondary); font-size: 12px; cursor: pointer; }
    .crypto-quiet-button:hover:not(:disabled) { color: var(--color-text); border-color: color-mix(in srgb, var(--color-text) 24%, var(--color-border)); }
    .crypto-primary-button { border-color: var(--color-accent); background: var(--color-accent); color: var(--color-accent-contrast); }
    .crypto-primary-button:hover:not(:disabled) { background: var(--color-accent-hover, var(--color-accent)); }
    .crypto-quiet-button:disabled, .crypto-primary-button:disabled { opacity: .5; cursor: default; }
    .crypto-file-workspace, .crypto-text-workspace { display: grid; grid-template-columns: minmax(0, 1fr); gap: 12px; align-items: start; }
    @media (min-width: 1024px) { .crypto-file-workspace { grid-template-columns: minmax(0, 1.15fr) minmax(260px, .85fr); } .crypto-text-workspace { grid-template-columns: 1fr 1fr; } }
    .crypto-queue, .crypto-results, .crypto-text-panel { padding: 14px; }
    .crypto-file-list, .crypto-result-list { display: flex; flex-direction: column; max-height: 21rem; margin-top: 12px; overflow: auto; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); }
    .crypto-file-row, .crypto-result-row { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 9px; padding: 8px 9px; border-top: 1px solid color-mix(in srgb, var(--color-border) 66%, transparent); color: var(--color-text); font-size: 12px; }
    .crypto-file-row:first-child, .crypto-result-row:first-child { border-top: 0; }
    .crypto-file-row :global(svg) { color: var(--color-muted); }
    .crypto-file-row > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .crypto-result-row { grid-template-columns: minmax(0, 1fr); }
    .crypto-result-row small { overflow: hidden; color: var(--color-muted); font-size: 10.5px; text-overflow: ellipsis; white-space: nowrap; }
    .crypto-result-row.is-error small { color: var(--color-error); }
    .crypto-empty { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 9px; min-height: 152px; margin-top: 12px; border: 1px dashed var(--color-border); border-radius: var(--radius-control, 8px); color: var(--color-muted); font-size: 12px; text-align: center; }
    .crypto-empty.is-quiet { min-height: 152px; border-style: solid; background: var(--color-panel-2); }
    .crypto-run-row { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
    .crypto-text-panel textarea { min-height: 260px; margin-top: 12px; padding: 10px; resize: vertical; font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace); font-size: 12px; line-height: 1.55; }
    .crypto-text-panel .crypto-primary-button { margin-top: 10px; }
    @media (max-width: 640px) { .crypto-command-spacer { display: none; } .crypto-credential-grid { grid-template-columns: 1fr; } .crypto-command-bar { align-items: stretch; } .crypto-segment { flex: 1; } .crypto-segment-button { flex: 1; } .crypto-keyfile-row { align-items: stretch; flex-direction: column; } }
</style>
