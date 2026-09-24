<script lang="ts">
    import { onMount } from 'svelte';
    import { Copy, Eye, EyeOff, FileKey2, KeyRound, Plus, Save, Server, ShieldCheck, Trash2, Upload } from '@lucide/svelte';
    import { ToolPage } from '$lib/ui';
    import { open } from '@tauri-apps/plugin-dialog';
    import {
        listKeys,
        generateKey,
        importKey,
        deleteKey,
        readConfig,
        writeConfig,
        readKnownHosts,
        removeKnownHost,
        sshDirPath,
        sshKeyList,
        sshKeyOperation,
        emptyHost,
        type SshKeyInfo,
        type SshConfig,
        type KnownHost,
    } from '$lib/stores/sshKeys';

    let tab = $state<'keys' | 'config' | 'known'>('keys');
    let keys = $derived($sshKeyList);
    let config = $state<SshConfig | null>(null);
    let known = $state<KnownHost[]>([]);
    let sshPath = $state('');
    let busy = $state(false);
    let message = $state<{ kind: 'ok' | 'err'; text: string } | null>(null);
    let operation = $derived($sshKeyOperation);
    let revealed = $state<Record<string, boolean>>({});
    let confirmDelete = $state<string | null>(null);

    // Generate form
    let gName = $state('id_ed25519');
    let gAlgo = $state<'ed25519' | 'rsa' | 'ecdsa'>('ed25519');
    let gBits = $state(4096);
    let gComment = $state('');
    let gPass = $state('');
    let gPass2 = $state('');

    function flash(kind: 'ok' | 'err', text: string) {
        message = { kind, text };
        setTimeout(() => (message = null), 4000);
    }

    async function loadKeys() {
        try {
            sshKeyList.set(await listKeys());
        } catch (e) {
            flash('err', String(e));
        }
    }
    async function loadConfig() {
        try {
            config = await readConfig();
        } catch (e) {
            flash('err', String(e));
        }
    }
    async function loadKnown() {
        try {
            known = await readKnownHosts();
        } catch (e) {
            flash('err', String(e));
        }
    }

    onMount(async () => {
        sshPath = await sshDirPath().catch(() => '~/.ssh');
    });

    $effect(() => {
        if (operation !== null) return;
        if (tab === 'keys') void loadKeys();
        else if (tab === 'config' && config === null) void loadConfig();
        else if (tab === 'known') void loadKnown();
    });

    // Suggest a default filename per algorithm.
    $effect(() => {
        if (gAlgo === 'ed25519' && (gName === 'id_rsa' || gName === 'id_ecdsa')) gName = 'id_ed25519';
        if (gAlgo === 'rsa' && (gName === 'id_ed25519' || gName === 'id_ecdsa')) gName = 'id_rsa';
        if (gAlgo === 'ecdsa' && (gName === 'id_ed25519' || gName === 'id_rsa')) gName = 'id_ecdsa';
    });

    const passMismatch = $derived(gPass.length > 0 && gPass !== gPass2);
    const canGenerate = $derived(gName.trim().length > 0 && !passMismatch && !busy && !operation);

    async function doGenerate() {
        if (!canGenerate || operation) return;
        sshKeyOperation.set('generate');
        busy = true;
        try {
            await generateKey({
                name: gName.trim(),
                algorithm: gAlgo,
                bits: gAlgo === 'rsa' ? gBits : undefined,
                comment: gComment.trim(),
                passphrase: gPass,
            });
            flash('ok', `Generated ${gName}`);
            gPass = '';
            gPass2 = '';
            gComment = '';
            await loadKeys();
        } catch (e) {
            flash('err', String(e));
        } finally {
            busy = false;
            sshKeyOperation.set(null);
        }
    }

    async function doImport() {
        if (busy || operation) return;
        const picked = await open({ multiple: false, title: 'Import an SSH private or public key' });
        if (!picked || Array.isArray(picked)) return;
        sshKeyOperation.set('import');
        busy = true;
        try {
            await importKey(picked);
            flash('ok', 'Key imported');
            await loadKeys();
        } catch (e) {
            flash('err', String(e));
        } finally {
            busy = false;
            sshKeyOperation.set(null);
        }

    }
    async function doDelete(name: string) {
        if (busy || operation) return;
        if (confirmDelete !== name) {
            confirmDelete = name;
            setTimeout(() => confirmDelete === name && (confirmDelete = null), 4000);
            return;
        }
        confirmDelete = null;
        sshKeyOperation.set('delete');
        busy = true;
        try {
            await deleteKey(name);
            flash('ok', `Deleted ${name}`);
            await loadKeys();
        } catch (e) {
            flash('err', String(e));
        } finally {
            busy = false;
            sshKeyOperation.set(null);
        }
    }

    async function copyPublic(key: SshKeyInfo) {
        try {
            await navigator.clipboard.writeText(key.publicKey);
            flash('ok', 'Public key copied');
        } catch {
            flash('err', 'Could not copy to clipboard');
        }
    }

    // ── Config editing ──
    function addHost() {
        if (operation) return;
        if (config) config.hosts = [...config.hosts, emptyHost()];
    }
    function removeHost(i: number) {
        if (operation) return;
        if (config) config.hosts = config.hosts.filter((_, idx) => idx !== i);
    }
    async function saveConfig() {
        if (!config || busy || operation) return;
        sshKeyOperation.set('save-config');
        busy = true;
        try {
            await writeConfig($state.snapshot(config));
            flash('ok', '~/.ssh/config saved');
        } catch (e) {
            flash('err', String(e));
        } finally {
            busy = false;
            sshKeyOperation.set(null);
        }
    }

    async function dropKnown(index: number) {
        if (busy || operation) return;
        sshKeyOperation.set('remove-known-host');
        busy = true;
        try {
            await removeKnownHost(index);
            flash('ok', 'Host key removed');
            await loadKnown();
        } catch (e) {
            flash('err', String(e));
        } finally {
            busy = false;
            sshKeyOperation.set(null);
        }
    }
</script>
<ToolPage
    icon={KeyRound}
    iconTint="#22c55e"
    title="SSH Key Manager"
    description="Generate, import, and manage SSH identities locally. Private key material stays on this device."
>
    <div class="ssh-workspace">
        {#if message}
            <div class="ssh-notice" class:is-error={message.kind === 'err'} role="status" aria-live="polite">{message.text}</div>
        {/if}

        <div class="ssh-command-bar">
            <div class="ssh-segment" role="group" aria-label="SSH workspace section">
                <button type="button" class="ssh-segment-button" class:is-active={tab === 'keys'} aria-pressed={tab === 'keys'} onclick={() => (tab = 'keys')}><KeyRound size={15} /> Keys</button>
                <button type="button" class="ssh-segment-button" class:is-active={tab === 'config'} aria-pressed={tab === 'config'} onclick={() => (tab = 'config')}><Server size={15} /> SSH config</button>
                <button type="button" class="ssh-segment-button" class:is-active={tab === 'known'} aria-pressed={tab === 'known'} onclick={() => (tab = 'known')}><ShieldCheck size={15} /> Known hosts</button>
            </div>
            <span class="ssh-path" title={sshPath}>Managed in <code>{sshPath}</code></span>
        </div>

        {#if tab === 'keys'}
            <div class="ssh-key-workspace">
                <section class="ssh-panel ssh-generate-panel" aria-labelledby="ssh-generate-title">
                    <div class="ssh-section-head">
                        <div>
                            <span class="ssh-eyebrow">New identity</span>
                            <h2 id="ssh-generate-title">Generate a key</h2>
                            <p>Create a dedicated identity for a service or device. Ed25519 is the recommended default.</p>
                        </div>
                        <button type="button" class="ssh-action-button" onclick={doImport} disabled={busy || operation !== null}><Upload size={15} /> Import existing</button>
                    </div>

                    <div class="ssh-generate-grid">
                        <div class="ssh-field ssh-field-wide">
                            <span>Algorithm</span>
                            <div class="ssh-option-group" role="group" aria-label="Key algorithm">
                                {#each [['ed25519', 'Ed25519'], ['rsa', 'RSA'], ['ecdsa', 'ECDSA']] as [val, lbl]}
                                    <button type="button" class="ssh-option" class:is-active={gAlgo === val} aria-pressed={gAlgo === val} onclick={() => (gAlgo = val as typeof gAlgo)}>{lbl}</button>
                                {/each}
                            </div>
                            {#if gAlgo === 'ed25519'}<small>Fast, small, and widely supported.</small>{/if}
                        </div>

                        {#if gAlgo === 'rsa'}
                            <label class="ssh-field">
                                <span>Key size</span>
                                <select bind:value={gBits} class="ssh-select">
                                    <option value={2048}>2048 bits</option>
                                    <option value={3072}>3072 bits</option>
                                    <option value={4096}>4096 bits (recommended)</option>
                                </select>
                            </label>
                        {/if}

                        <label class="ssh-field">
                            <span>File name</span>
                            <input bind:value={gName} autocomplete="off" />
                            <small>Saved in <code>~/.ssh</code>.</small>
                        </label>
                        <label class="ssh-field">
                            <span>Comment</span>
                            <input bind:value={gComment} placeholder="optional label" autocomplete="off" />
                            <small>For example, you@laptop.</small>
                        </label>
                        <label class="ssh-field">
                            <span>Passphrase</span>
                            <input type="password" bind:value={gPass} autocomplete="new-password" />
                            <small>Optional, but recommended for private keys.</small>
                        </label>
                        <label class="ssh-field">
                            <span>Confirm passphrase</span>
                            <input type="password" bind:value={gPass2} autocomplete="new-password" class:is-invalid={passMismatch} />
                            {#if passMismatch}<small class="ssh-error-text">Passphrases do not match.</small>{/if}
                        </label>
                    </div>

                    <div class="ssh-generate-footer">
                        <button type="button" class="ssh-primary-button" onclick={doGenerate} disabled={!canGenerate}><KeyRound size={16} /> {busy || operation ? 'Working...' : 'Generate key'}</button>
                        <span>Private key contents are never shown in this workspace.</span>
                    </div>
                </section>

                <aside class="ssh-security-note">
                    <ShieldCheck size={19} aria-hidden="true" />
                    <div>
                        <span>Private by design</span>
                        <p>Only key metadata, fingerprints, and public keys cross into the app interface. You can copy or inspect the public half without exposing private material.</p>
                    </div>
                </aside>
            </div>

            <section class="ssh-panel ssh-inventory" aria-labelledby="ssh-keys-title">
                <div class="ssh-section-head">
                    <div>
                        <span class="ssh-eyebrow">Identity inventory</span>
                        <h2 id="ssh-keys-title">Your keys <em>{keys.length}</em></h2>
                        <p>Review fingerprints and copy the public half when you need to register a key.</p>
                    </div>
                </div>

                {#if keys.length === 0}
                    <div class="ssh-empty"><FileKey2 size={22} /><span>No keys in <code>~/.ssh</code> yet. Generate or import one above.</span></div>
                {:else}
                    <div class="ssh-key-list">
                        {#each keys as key}
                            <article class="ssh-key-row">
                                <span class="ssh-key-mark" aria-hidden="true"><FileKey2 size={17} /></span>
                                <div class="ssh-key-copy">
                                    <div class="ssh-key-title-row">
                                        <strong>{key.name}</strong>
                                        <span class="ssh-tag">{key.algorithm}{key.bits ? ` ${key.bits}` : ''}</span>
                                        {#if key.encrypted}<span class="ssh-tag is-secure">passphrase protected</span>{/if}
                                        {#if !key.hasPrivate}<span class="ssh-tag is-warning">public only</span>{/if}
                                    </div>
                                    <code class="ssh-fingerprint" title={key.fingerprint}>{key.fingerprint}</code>
                                    {#if key.comment}<span class="ssh-key-comment">{key.comment}</span>{/if}
                                </div>
                                <div class="ssh-key-actions">
                                    <button type="button" class="ssh-action-button" onclick={() => copyPublic(key)}><Copy size={14} /> Copy public</button>
                                    <button type="button" class="ssh-action-button" onclick={() => (revealed[key.name] = !revealed[key.name])}>{#if revealed[key.name]}<EyeOff size={14} /> Hide{:else}<Eye size={14} /> View{/if}</button>
                                    <button type="button" class="ssh-action-button is-danger" class:is-confirm={confirmDelete === key.name} onclick={() => doDelete(key.name)} disabled={operation !== null}><Trash2 size={14} /> {operation === 'delete' ? 'Deleting...' : confirmDelete === key.name ? 'Confirm delete' : 'Delete'}</button>
                                </div>
                                {#if revealed[key.name]}
                                    <pre class="ssh-public-key">{key.publicKey}</pre>
                                {/if}
                            </article>
                        {/each}
                    </div>
                {/if}
            </section>
        {:else if tab === 'config'}
            {#if config}
                <section class="ssh-config-workspace" aria-labelledby="ssh-config-title">
                    <div class="ssh-section-head ssh-config-head">
                        <div>
                            <span class="ssh-eyebrow">Connection aliases</span>
                            <h2 id="ssh-config-title">SSH config <em>{config.hosts.length}</em></h2>
                            <p>Changes are written to <code>~/.ssh/config</code>. Existing unsupported directives stay preserved.</p>
                        </div>
                        <div class="ssh-section-actions">
                            <button type="button" class="ssh-action-button" onclick={addHost} disabled={busy || operation !== null}><Plus size={15} /> Add host</button>
                            <button type="button" class="ssh-primary-button" onclick={saveConfig} disabled={busy || operation !== null}><Save size={15} /> {busy || operation === 'save-config' ? 'Saving...' : 'Save config'}</button>
                        </div>
                    </div>

                    {#if config.hosts.length === 0}
                        <div class="ssh-empty"><Server size={22} /><span>No Host entries yet. Add one to create a memorable connection alias.</span></div>
                    {:else}
                        <div class="ssh-host-list">
                            {#each config.hosts as host, i}
                                <article class="ssh-host-card">
                                    <div class="ssh-host-card-head">
                                        <div class="ssh-host-label"><Server size={15} /><span>Host {i + 1}</span></div>
                                        <button type="button" class="ssh-action-button is-danger" onclick={() => removeHost(i)} disabled={busy || operation !== null} aria-label={`Remove host ${host.host || i + 1}`}><Trash2 size={14} /> Remove</button>
                                    </div>
                                    <div class="ssh-host-grid">
                                        <label class="ssh-field ssh-host-alias">
                                            <span>Alias</span>
                                            <input bind:value={host.host} placeholder="work-server" autocomplete="off" />
                                        </label>
                                        <label class="ssh-field">
                                            <span>Host name</span>
                                            <input bind:value={host.hostName} placeholder="github.com" autocomplete="off" />
                                        </label>
                                        <label class="ssh-field">
                                            <span>User</span>
                                            <input bind:value={host.user} placeholder="git" autocomplete="off" />
                                        </label>
                                        <label class="ssh-field">
                                            <span>Port</span>
                                            <input bind:value={host.port} placeholder="22" inputmode="numeric" autocomplete="off" />
                                        </label>
                                        <label class="ssh-field ssh-identity-file">
                                            <span>Identity file</span>
                                            <input bind:value={host.identityFile} placeholder="~/.ssh/id_ed25519" autocomplete="off" />
                                        </label>
                                    </div>
                                    {#if host.extra.length > 0}
                                        <div class="ssh-preserved-block">
                                            <span>Other directives preserved</span>
                                            {#each host.extra as line}<code>{line}</code>{/each}
                                        </div>
                                    {/if}
                                </article>
                            {/each}
                        </div>
                    {/if}

                    {#if config.preamble.trim()}
                        <details class="ssh-details">
                            <summary>Global options before the first Host are preserved</summary>
                            <pre>{config.preamble}</pre>
                        </details>
                    {/if}
                </section>
            {:else}
                <div class="ssh-empty is-loading"><Server size={22} /><span>Loading SSH config...</span></div>
            {/if}
        {:else}
            <section class="ssh-known-workspace" aria-labelledby="ssh-known-title">
                <div class="ssh-section-head">
                    <div>
                        <span class="ssh-eyebrow">Host trust</span>
                        <h2 id="ssh-known-title">Known hosts <em>{known.length}</em></h2>
                        <p>Remove a stale host key only after verifying the server's new fingerprint.</p>
                    </div>
                </div>
                <div class="ssh-security-note is-inline"><ShieldCheck size={18} aria-hidden="true" /><p>This resolves a changed-host warning without weakening SSH verification for future connections.</p></div>

                {#if known.length === 0}
                    <div class="ssh-empty"><ShieldCheck size={22} /><span>No known hosts recorded yet.</span></div>
                {:else}
                    <div class="ssh-known-list">
                        {#each known as entry (entry.index)}
                            <article class="ssh-known-row">
                                <span class="ssh-known-mark" aria-hidden="true"><Server size={16} /></span>
                                <div>
                                    <strong title={entry.hosts}>{entry.hosts}</strong>
                                    <code title={entry.fingerprint}>{entry.keyType} - {entry.fingerprint}</code>
                                </div>
                                <button type="button" class="ssh-action-button is-danger" onclick={() => dropKnown(entry.index)} disabled={operation !== null}>{operation === 'remove-known-host' ? 'Removing...' : 'Remove'}<Trash2 size={14} /></button>
                            </article>
                        {/each}
                    </div>
                {/if}
            </section>
        {/if}
    </div>
</ToolPage>

<style>
    .ssh-workspace { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
    .ssh-command-bar { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; padding-bottom: 10px; border-bottom: 1px solid var(--color-divider, var(--color-border)); }
    .ssh-segment { display: inline-flex; gap: 3px; padding: 3px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); background: var(--color-panel); }
    .ssh-segment-button { position: relative; display: inline-flex; align-items: center; gap: 6px; min-height: 32px; padding: 0 11px 0 16px; border: 0; border-radius: calc(var(--radius-control, 8px) - 2px); background: transparent; color: var(--color-text-secondary); font: inherit; font-size: 12px; cursor: pointer; }
    .ssh-segment-button:hover { color: var(--color-text); }
    .ssh-segment-button.is-active { background: var(--color-panel-2); color: var(--color-text); }
    .ssh-segment-button.is-active::before { content: ''; position: absolute; top: 7px; bottom: 7px; left: 5px; width: 3px; border-radius: 999px; background: var(--color-accent); }
    .ssh-segment-button:focus-visible, .ssh-option:focus-visible, .ssh-action-button:focus-visible, .ssh-primary-button:focus-visible, .ssh-field input:focus-visible, .ssh-select:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
    .ssh-path { margin-left: auto; overflow: hidden; color: var(--color-muted); font-size: 11.5px; text-overflow: ellipsis; white-space: nowrap; }
    .ssh-path code, .ssh-field code, .ssh-empty code, .ssh-section-head code { color: var(--color-text-secondary); font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace); }
    .ssh-notice { padding: 9px 12px; border-left: 2px solid var(--color-success); color: var(--color-text-secondary); font-size: 12.5px; }
    .ssh-notice.is-error { border-left-color: var(--color-error); color: var(--color-error); }
    .ssh-key-workspace { display: grid; grid-template-columns: minmax(0, 1fr); gap: 12px; align-items: start; }
    .ssh-panel, .ssh-config-workspace, .ssh-known-workspace { min-width: 0; border: 1px solid var(--color-border); border-radius: var(--radius-card, 12px); background: var(--color-panel); }
    .ssh-generate-panel, .ssh-inventory, .ssh-config-workspace, .ssh-known-workspace { padding: 16px; }
    .ssh-section-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 14px; }
    .ssh-section-head h2 { margin: 2px 0 0; color: var(--color-text); font-size: 15px; font-weight: 600; letter-spacing: -0.01em; }
    .ssh-section-head h2 em { margin-left: 5px; color: var(--color-muted); font-size: 11px; font-style: normal; font-weight: 500; }
    .ssh-section-head p { max-width: 62ch; margin: 4px 0 0; color: var(--color-muted); font-size: 12px; line-height: 1.45; }
    .ssh-eyebrow { color: var(--color-muted); font-size: 10.5px; font-weight: 600; letter-spacing: .05em; text-transform: uppercase; }
    .ssh-generate-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; margin-top: 16px; }
    .ssh-field { display: flex; flex-direction: column; gap: 6px; min-width: 0; color: var(--color-text-secondary); font-size: 12px; }
    .ssh-field-wide { grid-column: 1 / -1; }
    .ssh-field input, .ssh-select { box-sizing: border-box; width: 100%; min-height: 36px; padding: 0 10px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); background: var(--color-panel-2); color: var(--color-text); font: inherit; }
    .ssh-field input.is-invalid { border-color: var(--color-error); }
    .ssh-field small { color: var(--color-muted); font-size: 10.5px; line-height: 1.35; }
    .ssh-field .ssh-error-text { color: var(--color-error); }
    .ssh-option-group { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 4px; padding: 3px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); background: var(--color-panel-2); }
    .ssh-option { position: relative; min-height: 32px; padding: 0 9px 0 15px; border: 0; border-radius: calc(var(--radius-control, 8px) - 2px); background: transparent; color: var(--color-text-secondary); font: inherit; font-size: 12px; cursor: pointer; }
    .ssh-option:hover { color: var(--color-text); }
    .ssh-option.is-active { background: var(--color-panel); color: var(--color-text); }
    .ssh-option.is-active::before { content: ''; position: absolute; top: 7px; bottom: 7px; left: 5px; width: 3px; border-radius: 999px; background: var(--color-accent); }
    .ssh-generate-footer { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; margin-top: 16px; }
    .ssh-generate-footer > span { color: var(--color-muted); font-size: 11px; }
    .ssh-action-button, .ssh-primary-button { display: inline-flex; align-items: center; justify-content: center; gap: 6px; min-height: 34px; padding: 0 10px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); background: transparent; color: var(--color-text-secondary); font: inherit; font-size: 12px; cursor: pointer; }
    .ssh-action-button:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-text) 24%, var(--color-border)); color: var(--color-text); }
    .ssh-action-button.is-danger { color: var(--color-error); }
    .ssh-action-button.is-danger:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-error) 46%, var(--color-border)); }
    .ssh-action-button.is-confirm { border-color: var(--color-error); color: var(--color-error); }
    .ssh-primary-button { border-color: var(--color-accent); background: var(--color-accent); color: var(--color-accent-contrast); }
    .ssh-primary-button:hover:not(:disabled) { background: var(--color-accent-hover, var(--color-accent)); }
    .ssh-action-button:disabled, .ssh-primary-button:disabled { opacity: .5; cursor: default; }
    .ssh-security-note { display: flex; gap: 10px; padding: 13px 0 13px 12px; border-left: 2px solid var(--color-accent); color: var(--color-text-secondary); }
    .ssh-security-note :global(svg) { flex: none; color: var(--color-accent); }
    .ssh-security-note span { display: block; color: var(--color-text); font-size: 12px; font-weight: 600; }
    .ssh-security-note p { margin: 3px 0 0; color: var(--color-muted); font-size: 11.5px; line-height: 1.45; }
    .ssh-security-note.is-inline { align-items: flex-start; margin: 14px 0 0; }
    .ssh-security-note.is-inline p { margin: 0; }
    .ssh-inventory { display: flex; flex-direction: column; gap: 14px; }
    .ssh-key-list, .ssh-host-list, .ssh-known-list { display: flex; flex-direction: column; overflow: hidden; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); }
    .ssh-key-row { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 10px; align-items: center; padding: 11px; border-top: 1px solid color-mix(in srgb, var(--color-border) 66%, transparent); }
    .ssh-key-row:first-child, .ssh-host-card:first-child, .ssh-known-row:first-child { border-top: 0; }
    .ssh-key-mark, .ssh-known-mark { display: inline-flex; align-items: center; justify-content: center; width: 32px; height: 32px; border-radius: var(--radius-control, 8px); color: var(--color-muted); box-shadow: inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent), inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent); }
    .ssh-key-copy { min-width: 0; }
    .ssh-key-title-row { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; min-width: 0; }
    .ssh-key-title-row strong, .ssh-known-row strong { overflow: hidden; color: var(--color-text); font-size: 12.5px; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
    .ssh-tag { padding: 2px 6px; border: 1px solid var(--color-border); border-radius: 999px; color: var(--color-muted); font-size: 10px; line-height: 1.2; }
    .ssh-tag.is-secure { border-color: color-mix(in srgb, var(--color-success) 45%, var(--color-border)); color: var(--color-success); }
    .ssh-tag.is-warning { border-color: color-mix(in srgb, var(--color-warning) 45%, var(--color-border)); color: var(--color-warning); }
    .ssh-fingerprint, .ssh-key-comment { display: block; overflow: hidden; margin-top: 3px; color: var(--color-muted); font-size: 10.5px; text-overflow: ellipsis; white-space: nowrap; }
    .ssh-key-actions { display: flex; align-items: center; flex-wrap: wrap; justify-content: flex-end; gap: 6px; }
    .ssh-public-key { grid-column: 2 / -1; margin: 0; overflow: auto; padding: 10px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); background: var(--color-panel-2); color: var(--color-text-secondary); font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace); font-size: 11px; line-height: 1.45; white-space: pre-wrap; word-break: break-all; }
    .ssh-empty { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 9px; min-height: 148px; padding: 16px; border: 1px dashed var(--color-border); border-radius: var(--radius-control, 8px); color: var(--color-muted); font-size: 12px; text-align: center; }
    .ssh-empty :global(svg) { color: var(--color-muted); }
    .ssh-empty.is-loading { min-height: 180px; border-style: solid; }
    .ssh-config-workspace, .ssh-known-workspace { display: flex; flex-direction: column; gap: 14px; }
    .ssh-section-actions { display: flex; align-items: center; flex-wrap: wrap; justify-content: flex-end; gap: 8px; }
    .ssh-host-card { padding: 12px; border-top: 1px solid color-mix(in srgb, var(--color-border) 66%, transparent); }
    .ssh-host-card-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 12px; }
    .ssh-host-label { display: inline-flex; align-items: center; gap: 7px; color: var(--color-text); font-size: 12px; font-weight: 600; }
    .ssh-host-label :global(svg) { color: var(--color-muted); }
    .ssh-host-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }
    .ssh-host-alias, .ssh-identity-file { grid-column: span 1; }
    .ssh-preserved-block { display: flex; flex-direction: column; gap: 4px; margin-top: 12px; padding: 10px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); color: var(--color-muted); font-size: 10.5px; }
    .ssh-preserved-block > span { color: var(--color-text-secondary); font-size: 11px; font-weight: 600; }
    .ssh-preserved-block code { overflow: auto; font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace); white-space: pre-wrap; }
    .ssh-details { padding: 11px 12px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); color: var(--color-muted); font-size: 11.5px; }
    .ssh-details summary { cursor: pointer; color: var(--color-text-secondary); }
    .ssh-details pre { margin: 10px 0 0; overflow: auto; color: var(--color-muted); font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace); font-size: 11px; line-height: 1.45; white-space: pre-wrap; }
    .ssh-known-row { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 10px; align-items: center; padding: 11px; border-top: 1px solid color-mix(in srgb, var(--color-border) 66%, transparent); }
    .ssh-known-row > div { min-width: 0; }
    .ssh-known-row code { display: block; overflow: hidden; margin-top: 3px; color: var(--color-muted); font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace); font-size: 10.5px; text-overflow: ellipsis; white-space: nowrap; }
    @media (min-width: 1080px) { .ssh-key-workspace { grid-template-columns: minmax(0, 1fr) minmax(260px, .42fr); } .ssh-security-note { margin-top: 0; } }
    @media (max-width: 760px) { .ssh-path { display: none; } .ssh-generate-grid, .ssh-host-grid { grid-template-columns: 1fr; } .ssh-key-row, .ssh-known-row { grid-template-columns: auto minmax(0, 1fr); } .ssh-key-actions { grid-column: 1 / -1; justify-content: flex-start; } .ssh-public-key { grid-column: 1 / -1; } .ssh-section-head, .ssh-config-head { flex-direction: column; } .ssh-section-actions { justify-content: flex-start; } }
    @media (max-width: 500px) { .ssh-command-bar { align-items: stretch; } .ssh-segment { width: 100%; } .ssh-segment-button { flex: 1; justify-content: center; } .ssh-option-group { grid-template-columns: 1fr; } .ssh-generate-footer, .ssh-section-actions { align-items: stretch; flex-direction: column; } .ssh-generate-footer > button, .ssh-section-actions > button { width: 100%; } .ssh-key-actions { align-items: stretch; flex-direction: column; } .ssh-key-actions > button { width: 100%; } }
</style>
