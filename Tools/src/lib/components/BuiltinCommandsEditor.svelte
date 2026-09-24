<script lang="ts">
    /*
      BuiltinCommandsEditor — customize the trigger phrases of KeepItLocal's
      BUILT-IN voice commands (navigation + keyboard/mouse/window). The
      defaults live in commandRegistry; this edits a persisted override map
      (commandOverrides) applied at registry-build time. Per-command Reset
      drops one override; "Reset all" clears them. Saving hot-reloads a live
      command-mode session so changes take effect immediately.

      Commands are grouped by category in collapsible sections (navigation
      open by default) so the ~70 input primitives don't overwhelm.
    */
    import { onMount } from 'svelte';
    import { RotateCcw, ChevronDown } from '@lucide/svelte';
    import { Button } from '$lib/ui';
    import { toast } from '$lib/stores/toasts';
    import { confirm } from '$lib/stores/confirmDialog';
    import { _ } from 'svelte-i18n';
    import {
        getCuratedBuiltinCommands,
        type CuratedBuiltinCommand,
    } from '$lib/stores/commandRegistry';
    import {
        commandOverrides,
        getCommandPhraseOverrides,
        replaceAllCommandOverrides,
        resetAllCommandOverrides,
        type CommandOverrides,
    } from '$lib/stores/commandOverrides';

    interface Edit {
        en: string;
        ka: string;
    }

    const curated = getCuratedBuiltinCommands();
    const CATEGORY_ORDER = ['navigation', 'window', 'keyboard', 'mouse', 'letters', 'other'];
    const groups = CATEGORY_ORDER.map((cat) => ({
        cat,
        items: curated.filter((c) => c.category === cat),
    })).filter((g) => g.items.length > 0);

    function buildSeed(): Record<string, Edit> {
        const ov = getCommandPhraseOverrides();
        const next: Record<string, Edit> = {};
        for (const c of curated) {
            const o = ov[c.id];
            next[c.id] = {
                en: (o?.en ?? c.defaults.en).join(', '),
                ka: (o?.ka ?? c.defaults.ka).join(', '),
            };
        }
        return next;
    }
    function reseed() {
        edits = buildSeed();
    }

    // Comma-joined phrases per command id, seeded synchronously (so the
    // template's bind:value never hits an undefined row before onMount).
    let edits = $state<Record<string, Edit>>(buildSeed());
    let saving = $state(false);
    let openGroups = $state<Record<string, boolean>>({ navigation: true });

    function split(value: string): string[] {
        return value
            .split(',')
            .map((p) => p.trim())
            .filter((p) => p.length > 0);
    }
    function eq(a: string[], b: string[]): boolean {
        return a.length === b.length && a.every((v, i) => v === b[i]);
    }
    function isDefault(c: CuratedBuiltinCommand): boolean {
        const e = edits[c.id];
        if (!e) return true;
        return eq(split(e.en), c.defaults.en) && eq(split(e.ka), c.defaults.ka);
    }

    /** The override map the current edits would produce (non-default only). */
    function computeMap(): CommandOverrides {
        const map: CommandOverrides = {};
        for (const c of curated) {
            if (isDefault(c)) continue;
            const en = split(edits[c.id].en);
            const ka = split(edits[c.id].ka);
            if (en.length || ka.length) map[c.id] = { en, ka };
        }
        return map;
    }

    // Dirty vs the persisted snapshot ($commandOverrides keeps this reactive).
    const dirty = $derived(JSON.stringify(computeMap()) !== JSON.stringify($commandOverrides));

    function toggleGroup(cat: string) {
        openGroups = { ...openGroups, [cat]: !openGroups[cat] };
    }
    function resetOne(c: CuratedBuiltinCommand) {
        edits = {
            ...edits,
            [c.id]: { en: c.defaults.en.join(', '), ka: c.defaults.ka.join(', ') },
        };
    }
    async function save() {
        saving = true;
        try {
            await replaceAllCommandOverrides(computeMap());
            toast($_('settings.voiceSection.cmdSaved'), 'success', 2500);
        } catch (error) {
            toast(String(error), 'error', 5000);
        } finally {
            saving = false;
        }
    }
    async function resetAll() {
        const ok = await confirm($_('settings.voiceSection.biResetAllConfirm'), {
            title: $_('settings.voiceSection.biResetAll'),
            confirmLabel: $_('settings.voiceSection.biResetAll'),
            danger: true,
        });
        if (!ok) return;
        await resetAllCommandOverrides();
        reseed();
        toast($_('settings.voiceSection.biResetDone'), 'success', 2500);
    }

    function categoryLabel(cat: string): string {
        return $_(`settings.voiceSection.biCat_${cat}`);
    }

    // Re-seed on mount in case overrides finished loading after init (the
    // +layout loader runs at app start, but Settings may mount before that).
    onMount(reseed);
</script>

<div class="bce">
    <p class="bce-desc">{$_('settings.voiceSection.biDesc')}</p>

    <div class="bce-groups">
        {#each groups as group (group.cat)}
            <div class="bce-group">
                <button class="bce-group-head" onclick={() => toggleGroup(group.cat)}>
                    <ChevronDown class={`bce-chevron ${openGroups[group.cat] ? 'open' : ''}`} />
                    <span>{categoryLabel(group.cat)}</span>
                    <span class="bce-count">{group.items.length}</span>
                </button>
                {#if openGroups[group.cat]}
                    <div class="bce-rows">
                        {#each group.items as c (c.id)}
                            <div class="bce-row">
                                <div class="bce-row-head">
                                    <span class="bce-title">{c.title}</span>
                                    {#if !isDefault(c)}
                                        <button
                                            class="bce-reset"
                                            onclick={() => resetOne(c)}
                                            title={$_('settings.voiceSection.biReset')}
                                        >
                                            <RotateCcw size={13} />
                                            {$_('settings.voiceSection.biReset')}
                                        </button>
                                    {/if}
                                </div>
                                <div class="bce-fields">
                                    <label class="bce-field">
                                        <span class="bce-lang">{$_('settings.voiceSection.biEn')}</span>
                                        <input
                                            class="bce-input"
                                            bind:value={edits[c.id].en}
                                            spellcheck="false"
                                        />
                                    </label>
                                    <label class="bce-field">
                                        <span class="bce-lang">{$_('settings.voiceSection.biKa')}</span>
                                        <input
                                            class="bce-input"
                                            bind:value={edits[c.id].ka}
                                            spellcheck="false"
                                        />
                                    </label>
                                </div>
                            </div>
                        {/each}
                    </div>
                {/if}
            </div>
        {/each}
    </div>

    <div class="bce-actions">
        <Button variant="primary" size="sm" onclick={save} disabled={!dirty || saving}>
            {$_('settings.voiceSection.cmdSave')}
        </Button>
        <Button variant="ghost" size="sm" icon={RotateCcw} onclick={resetAll}>
            {$_('settings.voiceSection.biResetAll')}
        </Button>
    </div>
</div>

<style>
    .bce {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .bce-desc {
        margin: 0;
        font-size: 13px;
        color: var(--color-text-secondary);
        line-height: 1.5;
    }
    .bce-groups {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .bce-group {
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        overflow: hidden;
    }
    .bce-group-head {
        display: flex;
        align-items: center;
        gap: 8px;
        width: 100%;
        padding: 10px 12px;
        background: var(--color-panel-2);
        border: none;
        color: var(--color-text);
        font-size: 13px;
        font-weight: 600;
        cursor: pointer;
        text-align: left;
    }
    .bce-group-head :global(.bce-chevron) {
        width: 15px;
        height: 15px;
        color: var(--color-muted);
        transition: transform var(--dur-micro, 130ms) ease;
    }
    .bce-group-head :global(.bce-chevron.open) {
        transform: rotate(180deg);
    }
    .bce-count {
        margin-left: auto;
        font-size: 11px;
        font-weight: 500;
        color: var(--color-muted);
    }
    .bce-rows {
        display: flex;
        flex-direction: column;
        gap: 10px;
        padding: 12px;
    }
    .bce-row {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .bce-row-head {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .bce-title {
        font-size: 12px;
        color: var(--color-text-secondary);
    }
    .bce-reset {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        margin-left: auto;
        padding: 1px 6px;
        background: transparent;
        border: none;
        color: var(--color-accent);
        font-size: 11px;
        cursor: pointer;
    }
    .bce-fields {
        display: flex;
        gap: 8px;
        flex-wrap: wrap;
    }
    .bce-field {
        display: flex;
        align-items: center;
        gap: 6px;
        flex: 1 1 220px;
        min-width: 0;
    }
    .bce-lang {
        flex: none;
        width: 22px;
        font-size: 11px;
        color: var(--color-muted);
        text-transform: uppercase;
    }
    .bce-input {
        flex: 1;
        min-width: 0;
        height: 30px;
        padding: 0 9px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        color: var(--color-text);
        font-size: 13px;
        outline: none;
    }
    .bce-input:focus {
        border-color: var(--color-accent);
    }
    .bce-actions {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-wrap: wrap;
        margin-top: 4px;
    }
</style>
