<script lang="ts">
    /*
      VoiceCommandsEditor — in-app add/edit/delete for the user's custom
      voice commands. Reads `voice-commands.txt` via `voice_read_user_commands`,
      presents each command as an editable row, and writes the file back via
      `voice_write_user_commands`. The on-disk watcher then hot-reloads the
      command registry, so saved commands work immediately in Command Mode /
      Push-to-Talk.

      The file format is `phrase = key <shortcut>` or `phrase = url <address>`,
      with optional `[app: name]` scope headers. This editor owns the structured
      view; the "Open file" button drops to the raw text for power users (and
      hand-written comments, which the structured save does not preserve).
    */
    import { onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { Plus, Trash2, Save, FolderOpen, Loader2 } from '@lucide/svelte';
    import { Button, Select } from '$lib/ui';
    import { toast } from '$lib/stores/toasts';
    import { confirm } from '$lib/stores/confirmDialog';
    import { _ } from 'svelte-i18n';

    type ActionType = 'key' | 'url';
    interface Row {
        id: number;
        phrase: string;
        type: ActionType;
        target: string;
        app: string;
    }

    let rows = $state<Row[]>([]);
    let baseline = $state('');
    let loading = $state(true);
    let saving = $state(false);
    let nextId = 0;

    const typeOptions = $derived([
        { value: 'key', label: $_('settings.voiceSection.cmdActionKey') },
        { value: 'url', label: $_('settings.voiceSection.cmdActionUrl') },
    ]);

    // Dirty = the current rows serialize to something other than what we last
    // loaded/saved. baseline is the serialized form so reformatting on load
    // doesn't read as an unsaved change.
    const dirty = $derived(serialize(rows) !== baseline);

    /** Parse the raw command file into editable rows, tracking [app: x] scope.
     *  Comment / blank / header lines are skipped (their scope is applied). */
    function parse(text: string): Row[] {
        const out: Row[] = [];
        let app = '';
        for (const raw of text.split(/\r?\n/)) {
            const line = raw.trim();
            if (line.length === 0 || line.startsWith('#')) continue;
            if (line.startsWith('[')) {
                const m = line.match(/^\[\s*app\s*:\s*(.+?)\s*\]$/i);
                if (m) {
                    const n = m[1].trim().toLowerCase();
                    app = n === '*' || n === 'any' ? '' : n.replace(/\.exe$/, '');
                } else if (/^\[\s*global\s*\]$/i.test(line)) {
                    app = '';
                }
                continue;
            }
            const eq = line.indexOf('=');
            if (eq < 0) continue;
            const phrase = line.slice(0, eq).trim();
            const spec = line.slice(eq + 1).trim();
            const sp = spec.indexOf(' ');
            const kind = (sp < 0 ? spec : spec.slice(0, sp)).toLowerCase();
            const target = sp < 0 ? '' : spec.slice(sp + 1).trim();
            if ((kind === 'key' || kind === 'url') && phrase.length > 0) {
                out.push({ id: nextId++, phrase, type: kind, target, app });
            }
        }
        return out;
    }

    /** Serialize editable rows back to the file format, grouped by app scope
     *  (global first). Blank/incomplete rows are dropped. */
    function serialize(list: Row[]): string {
        const valid = list.filter((r) => r.phrase.trim() && r.target.trim());
        const groups = new Map<string, Row[]>();
        for (const r of valid) {
            const key = r.app.trim().toLowerCase().replace(/\.exe$/, '');
            if (!groups.has(key)) groups.set(key, []);
            groups.get(key)!.push(r);
        }
        const lines: string[] = [
            '# KeepItLocal voice commands — managed by the in-app editor.',
            '# phrase = key <shortcut>   |   phrase = url <address>',
            '',
        ];
        const keys = [...groups.keys()].sort((a, b) =>
            a === '' ? -1 : b === '' ? 1 : a.localeCompare(b),
        );
        for (const key of keys) {
            lines.push(key === '' ? '[app: *]' : `[app: ${key}]`);
            for (const r of groups.get(key)!) {
                lines.push(`${r.phrase.trim()} = ${r.type} ${r.target.trim()}`);
            }
            lines.push('');
        }
        return lines.join('\n');
    }

    async function load() {
        loading = true;
        try {
            const text = await invoke<string>('voice_read_user_commands');
            rows = parse(text);
            baseline = serialize(rows);
        } catch (error) {
            toast(String(error), 'error', 4500);
            rows = [];
            baseline = serialize(rows);
        } finally {
            loading = false;
        }
    }

    function addRow() {
        rows = [...rows, { id: nextId++, phrase: '', type: 'key', target: '', app: '' }];
    }

    async function deleteRow(row: Row) {
        const label = row.phrase.trim() || $_('settings.voiceSection.cmdThisCommand');
        const ok = await confirm(
            $_('settings.voiceSection.cmdDeleteConfirm', { values: { phrase: label } }),
            {
                title: $_('settings.voiceSection.cmdDeleteTitle'),
                confirmLabel: $_('settings.voiceSection.cmdDeleteCta'),
                danger: true,
            },
        );
        if (!ok) return;
        rows = rows.filter((r) => r.id !== row.id);
    }

    async function save() {
        // A phrase with no action (or vice-versa) is a mistake, not an empty
        // row to silently drop — flag it instead of writing a broken file.
        const broken = rows.find(
            (r) => (r.phrase.trim() === '') !== (r.target.trim() === ''),
        );
        if (broken) {
            toast($_('settings.voiceSection.cmdIncomplete'), 'error', 4500);
            return;
        }
        saving = true;
        try {
            const contents = serialize(rows);
            await invoke('voice_write_user_commands', { contents });
            // Re-parse from the serialized form so ids/order match the saved
            // file and the dirty flag clears.
            rows = parse(contents);
            baseline = serialize(rows);
            toast($_('settings.voiceSection.cmdSaved'), 'success', 2500);
        } catch (error) {
            toast(String(error), 'error', 5000);
        } finally {
            saving = false;
        }
    }

    async function openRawFile() {
        try {
            await invoke('voice_open_user_commands_file');
        } catch (error) {
            toast(String(error), 'error', 4500);
        }
    }

    onMount(load);
</script>

<div class="vce">
    <p class="vce-desc">{$_('settings.voiceSection.cmdDesc')}</p>

    {#if loading}
        <div class="vce-loading"><Loader2 class="vce-spin" /> {$_('settings.voiceSection.cmdLoading')}</div>
    {:else}
        {#if rows.length === 0}
            <div class="vce-empty">{$_('settings.voiceSection.cmdEmpty')}</div>
        {/if}

        <div class="vce-rows">
            {#each rows as row (row.id)}
                <div class="vce-row">
                    <input
                        class="vce-input vce-phrase"
                        bind:value={row.phrase}
                        placeholder={$_('settings.voiceSection.cmdPhrasePlaceholder')}
                        spellcheck="false"
                    />
                    <span class="vce-arrow">→</span>
                    <div class="vce-type">
                        <Select bind:value={row.type} options={typeOptions} size="sm" />
                    </div>
                    <input
                        class="vce-input vce-target"
                        bind:value={row.target}
                        placeholder={row.type === 'key'
                            ? $_('settings.voiceSection.cmdKeyPlaceholder')
                            : $_('settings.voiceSection.cmdUrlPlaceholder')}
                        spellcheck="false"
                    />
                    <input
                        class="vce-input vce-app"
                        bind:value={row.app}
                        placeholder={$_('settings.voiceSection.cmdAppPlaceholder')}
                        title={$_('settings.voiceSection.cmdAppHint')}
                        spellcheck="false"
                    />
                    <button
                        class="vce-del"
                        onclick={() => void deleteRow(row)}
                        title={$_('settings.voiceSection.cmdDeleteTitle')}
                        aria-label={$_('settings.voiceSection.cmdDeleteTitle')}
                    >
                        <Trash2 size={15} />
                    </button>
                </div>
            {/each}
        </div>

        <div class="vce-actions">
            <Button variant="secondary" size="sm" icon={Plus} onclick={addRow}>
                {$_('settings.voiceSection.cmdAdd')}
            </Button>
            <Button
                variant="primary"
                size="sm"
                icon={saving ? Loader2 : Save}
                onclick={save}
                disabled={!dirty || saving}
            >
                {$_('settings.voiceSection.cmdSave')}
            </Button>
            <Button variant="ghost" size="sm" icon={FolderOpen} onclick={openRawFile}>
                {$_('settings.voiceSection.cmdOpenFile')}
            </Button>
        </div>
    {/if}
</div>

<style>
    .vce {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .vce-desc {
        margin: 0;
        font-size: 13px;
        color: var(--color-text-secondary);
        line-height: 1.5;
    }
    .vce-loading,
    .vce-empty {
        display: flex;
        align-items: center;
        gap: 8px;
        font-size: 13px;
        color: var(--color-muted);
        padding: 8px 0;
    }
    .vce-loading :global(.vce-spin) {
        width: 16px;
        height: 16px;
        animation: vce-spin 1s linear infinite;
    }
    @keyframes vce-spin {
        to {
            transform: rotate(360deg);
        }
    }
    .vce-rows {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .vce-row {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-wrap: wrap;
    }
    .vce-input {
        height: 32px;
        padding: 0 10px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        color: var(--color-text);
        font-size: 13px;
        min-width: 0;
        outline: none;
    }
    .vce-input:focus {
        border-color: var(--color-accent);
    }
    .vce-phrase {
        flex: 2 1 130px;
    }
    .vce-target {
        flex: 2 1 130px;
    }
    .vce-app {
        flex: 1 1 96px;
    }
    .vce-type {
        flex: none;
    }
    .vce-arrow {
        flex: none;
        color: var(--color-muted);
        font-size: 13px;
    }
    .vce-del {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 32px;
        height: 32px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: transparent;
        color: var(--color-muted);
        cursor: pointer;
        transition:
            color var(--dur-micro, 130ms) ease,
            border-color var(--dur-micro, 130ms) ease;
    }
    .vce-del:hover {
        color: var(--color-error);
        border-color: color-mix(in srgb, var(--color-error) 40%, var(--color-border));
    }
    .vce-actions {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-wrap: wrap;
        margin-top: 4px;
    }
</style>
