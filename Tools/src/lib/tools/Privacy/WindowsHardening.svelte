<script lang="ts">
    /*
      Privacy Hardening — the WRITE counterpart to Privacy Audit. Reversible,
      no-admin Windows privacy toggles (HKCU) plus a guide tier for the
      machine-wide tweaks that genuinely need admin (we never run those; we
      show steps + a copyable command). State lives in a module store so it
      survives the workspace's nav unmount.
    */
    import { onMount } from 'svelte';
    import { get } from 'svelte/store';
    import { ToolPage, Toggle, Button, ErrorState, LoadingState } from '$lib/ui';
    import { save } from '@tauri-apps/plugin-dialog';
    import { toast } from '$lib/stores/toasts';
    import {
        ShieldCheck,
        RefreshCw,
        RotateCcw,
        Download,
        ChevronDown,
        Copy,
        Check,
        WifiOff,
        Lock,
        Megaphone,
        Activity,
        Search,
        Sparkles,
        AppWindow,
    } from '@lucide/svelte';
    import {
        tweaks,
        loaded,
        loading,
        errorMsg,
        busy,
        loadHardeningState,
        setTweakApplied,
        revertAll,
        exportBackup,
        type HardeningTweak,
    } from '$lib/stores/windowsHardening';

    const CATEGORY_ORDER = [
        'Advertising & tracking',
        'Telemetry & diagnostics',
        'Cortana & search',
        'Suggested content & ads',
        'Lock screen & tips',
        'App behavior',
    ];
    const CATEGORY_ICON: Record<string, any> = {
        'Advertising & tracking': Megaphone,
        'Telemetry & diagnostics': Activity,
        'Cortana & search': Search,
        'Suggested content & ads': Sparkles,
        'Lock screen & tips': Lock,
        'App behavior': AppWindow,
    };

    let open = $state<Record<string, boolean>>({});
    let stepsOpen = $state<Record<string, boolean>>({});
    let copied = $state<Record<string, boolean>>({});
    let confirmRevert = $state(false);

    const grouped = $derived(
        CATEGORY_ORDER.map((category) => ({
            category,
            items: $tweaks.filter((t) => t.category === category),
        })).filter((g) => g.items.length > 0),
    );

    function appliedCount(items: HardeningTweak[]): { on: number; total: number } {
        const apply = items.filter((t) => t.tier === 'apply');
        return { on: apply.filter((t) => t.applied).length, total: apply.length };
    }

    function isOpen(cat: string): boolean {
        return open[cat] ?? true;
    }

    async function onToggle(t: HardeningTweak, value: boolean) {
        try {
            await setTweakApplied(t.id, value);
        } catch {
            toast(`Couldn't ${value ? 'apply' : 'revert'} "${t.title}"`, 'error');
        }
    }

    async function copyCommand(t: HardeningTweak) {
        if (!t.command) return;
        try {
            await navigator.clipboard.writeText(t.command);
            copied = { ...copied, [t.id]: true };
            setTimeout(() => {
                copied = { ...copied, [t.id]: false };
            }, 1600);
        } catch {
            toast('Could not copy to clipboard', 'error');
        }
    }

    async function doExport() {
        const path = await save({
            defaultPath: 'privacy-hardening-backup.json',
            filters: [{ name: 'JSON', extensions: ['json'] }],
        });
        if (!path) return;
        try {
            await exportBackup(path);
            toast('Backup saved', 'success');
        } catch (e) {
            toast(`Could not save backup: ${e}`, 'error');
        }
    }

    async function doRevertAll() {
        confirmRevert = false;
        try {
            await revertAll();
            toast('Reverted all applied tweaks', 'success');
        } catch {
            toast("Couldn't revert all tweaks", 'error');
        }
    }

    onMount(() => {
        if (!get(loaded)) void loadHardeningState();
    });
</script>

<ToolPage
    icon={ShieldCheck}
    iconTint="#10b981"
    title="Harden your Windows privacy"
    description="Reversible, no-admin toggles that turn off Windows telemetry, ad tracking, and suggested-content ads. Every change is captured before it's applied, so you can undo it exactly. The few machine-wide tweaks that genuinely need admin come with a step-by-step guide instead."
    width="wide"
    fill={false}
>
    <div class="wh-toolbar">
        <span class="wh-chip"><WifiOff class="wh-chip-ico" /> 100% local · no network</span>
        <span class="wh-spacer"></span>
        <Button
            variant="ghost"
            size="sm"
            icon={RefreshCw}
            onclick={() => void loadHardeningState()}
            disabled={$loading}>Refresh</Button
        >
        <Button variant="ghost" size="sm" icon={Download} onclick={doExport}>Export backup</Button>
        {#if confirmRevert}
            <span class="wh-confirm">
                <span class="wh-confirm-q">Revert everything?</span>
                <Button variant="ghost" size="sm" onclick={() => (confirmRevert = false)}
                    >Cancel</Button
                >
                <Button variant="danger" size="sm" onclick={doRevertAll}>Revert all</Button>
            </span>
        {:else}
            <Button
                variant="ghost"
                size="sm"
                icon={RotateCcw}
                onclick={() => (confirmRevert = true)}>Revert all</Button
            >
        {/if}
    </div>

    {#if $errorMsg}
        <ErrorState
            title="Couldn't read your settings"
            description={$errorMsg ?? ''}
            retry={() => void loadHardeningState()}
        />
    {/if}

    {#if $loading && !$loaded}
        <LoadingState label="Reading your current Windows settings…" />
    {:else}
        <div class="wh-cats">
            {#each grouped as g (g.category)}
                {@const CatIcon = CATEGORY_ICON[g.category]}
                {@const count = appliedCount(g.items)}
                <section class="wh-cat">
                    <button
                        class="wh-cat-head"
                        type="button"
                        onclick={() => (open = { ...open, [g.category]: !isOpen(g.category) })}
                        aria-expanded={isOpen(g.category)}
                    >
                        <span class="wh-cat-ico" aria-hidden="true"><CatIcon class="wh-cat-ico-svg" /></span>
                        <span class="wh-cat-title">{g.category}</span>
                        {#if count.total > 0}
                            <span class="wh-cat-count">{count.on} of {count.total} on</span>
                        {/if}
                        <ChevronDown class="wh-chevron {isOpen(g.category) ? 'is-open' : ''}" />
                    </button>
                    {#if isOpen(g.category)}
                        <div class="wh-rows">
                            {#each g.items as t (t.id)}
                                <div class="wh-row" class:is-applied={t.tier === 'apply' && t.applied}>
                                    <div class="wh-row-main">
                                        <div class="wh-row-title-line">
                                            <span class="wh-row-title">{t.title}</span>
                                            <span class="wh-risk wh-risk-{t.risk}">{t.risk}</span>
                                            {#if t.tier === 'guide'}
                                                <span class="wh-admin"
                                                    ><Lock class="wh-admin-ico" /> Needs admin</span
                                                >
                                            {/if}
                                        </div>
                                        <p class="wh-row-detail">{t.detail}</p>
                                        {#if t.tier === 'guide'}
                                            <button
                                                class="wh-steps-toggle"
                                                type="button"
                                                onclick={() =>
                                                    (stepsOpen = {
                                                        ...stepsOpen,
                                                        [t.id]: !stepsOpen[t.id],
                                                    })}
                                            >
                                                {stepsOpen[t.id] ? 'Hide steps' : 'Show me how'}
                                            </button>
                                            {#if stepsOpen[t.id]}
                                                <div class="wh-guide">
                                                    <ol class="wh-guide-steps">
                                                        {#each t.steps as s, i (i)}
                                                            <li>{s}</li>
                                                        {/each}
                                                    </ol>
                                                    {#if t.command}
                                                        <div class="wh-cmd">
                                                            <code class="wh-cmd-text">{t.command}</code>
                                                            <button
                                                                class="wh-cmd-copy"
                                                                type="button"
                                                                onclick={() => copyCommand(t)}
                                                                aria-label="Copy command"
                                                            >
                                                                {#if copied[t.id]}
                                                                    <Check class="wh-cmd-ico" />
                                                                {:else}
                                                                    <Copy class="wh-cmd-ico" />
                                                                {/if}
                                                            </button>
                                                        </div>
                                                    {/if}
                                                </div>
                                            {/if}
                                        {/if}
                                    </div>
                                    {#if t.tier === 'apply'}
                                        <div class="wh-row-ctl">
                                            <Toggle
                                                checked={t.applied}
                                                onchange={(v) => onToggle(t, v)}
                                                ariaLabel={t.title}
                                                disabled={!!$busy[t.id]}
                                            />
                                        </div>
                                    {/if}
                                </div>
                            {/each}
                        </div>
                    {/if}
                </section>
            {/each}
        </div>
    {/if}
</ToolPage>

<style>
    .wh-toolbar {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 8px;
        margin-bottom: 16px;
    }
    .wh-spacer {
        flex: 1;
    }
    .wh-chip {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-size: 12px;
        color: var(--color-text-secondary);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 999px;
        padding: 4px 10px;
    }
    .wh-chip :global(.wh-chip-ico) {
        width: 13px;
        height: 13px;
        color: var(--color-accent);
    }
    .wh-confirm {
        display: inline-flex;
        align-items: center;
        gap: 8px;
    }
    .wh-confirm-q {
        font-size: 13px;
        color: var(--color-text-secondary);
    }
    .wh-cats {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .wh-cat {
        border: 1px solid var(--color-border);
        border-radius: 12px;
        background: var(--color-panel);
        overflow: hidden;
    }
    .wh-cat-head {
        display: flex;
        align-items: center;
        gap: 10px;
        width: 100%;
        padding: 12px 14px;
        border: none;
        background: transparent;
        cursor: pointer;
        text-align: left;
        color: var(--color-text);
    }
    .wh-cat-head:hover {
        background: var(--color-panel-2);
    }
    .wh-cat-ico {
        display: grid;
        place-items: center;
        width: 28px;
        height: 28px;
        border-radius: 8px;
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
        flex: none;
    }
    .wh-cat-ico :global(.wh-cat-ico-svg) {
        width: 15px;
        height: 15px;
        color: var(--color-accent);
    }
    .wh-cat-title {
        font-size: 14px;
        font-weight: 600;
        flex: 1;
    }
    .wh-cat-count {
        font-size: 12px;
        color: var(--color-text-secondary);
    }
    .wh-cat-head :global(.wh-chevron) {
        width: 16px;
        height: 16px;
        color: var(--color-text-secondary);
        transition: transform 150ms ease;
    }
    .wh-cat-head :global(.wh-chevron.is-open) {
        transform: rotate(180deg);
    }
    .wh-rows {
        border-top: 1px solid var(--color-border);
    }
    .wh-row {
        position: relative;
        display: flex;
        align-items: flex-start;
        gap: 14px;
        padding: 12px 14px 12px 16px;
        border-bottom: 1px solid var(--color-border);
    }
    .wh-row:last-child {
        border-bottom: none;
    }
    .wh-row.is-applied {
        background: var(--color-panel-2);
    }
    .wh-row.is-applied::before {
        content: '';
        position: absolute;
        left: 0;
        top: 10px;
        bottom: 10px;
        width: 3px;
        border-radius: 0 2px 2px 0;
        background: var(--color-accent);
    }
    .wh-row-main {
        flex: 1;
        min-width: 0;
    }
    .wh-row-title-line {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 8px;
    }
    .wh-row-title {
        font-size: 13.5px;
        font-weight: 500;
        color: var(--color-text);
    }
    .wh-risk {
        font-size: 10.5px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.03em;
        border-radius: 5px;
        padding: 1px 6px;
    }
    .wh-risk-low {
        color: var(--color-text-secondary);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
    }
    .wh-risk-medium {
        color: var(--color-warning);
        background: color-mix(in srgb, var(--color-warning) 16%, transparent);
    }
    .wh-risk-high {
        color: var(--color-error);
        background: color-mix(in srgb, var(--color-error) 16%, transparent);
    }
    .wh-admin {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        font-size: 11px;
        color: var(--color-text-secondary);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 5px;
        padding: 1px 6px;
    }
    .wh-admin :global(.wh-admin-ico) {
        width: 11px;
        height: 11px;
    }
    .wh-row-detail {
        margin: 4px 0 0;
        font-size: 12.5px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }
    .wh-steps-toggle {
        margin-top: 6px;
        border: none;
        background: transparent;
        padding: 0;
        font-size: 12.5px;
        font-weight: 500;
        color: var(--color-accent);
        cursor: pointer;
    }
    .wh-steps-toggle:hover {
        text-decoration: underline;
    }
    .wh-guide {
        margin-top: 8px;
        padding: 10px 12px;
        border: 1px solid var(--color-border);
        border-radius: 8px;
        background: var(--color-panel-2);
    }
    .wh-guide-steps {
        margin: 0;
        padding-left: 18px;
        font-size: 12.5px;
        line-height: 1.6;
        color: var(--color-text);
    }
    .wh-cmd {
        display: flex;
        align-items: center;
        gap: 8px;
        margin-top: 10px;
    }
    .wh-cmd-text {
        flex: 1;
        min-width: 0;
        overflow-x: auto;
        white-space: nowrap;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 12px;
        color: var(--color-text);
        background: var(--color-bg, var(--color-panel));
        border: 1px solid var(--color-border);
        border-radius: 6px;
        padding: 6px 8px;
    }
    .wh-cmd-copy {
        flex: none;
        display: grid;
        place-items: center;
        width: 30px;
        height: 30px;
        border: 1px solid var(--color-border);
        border-radius: 6px;
        background: var(--color-panel);
        color: var(--color-text-secondary);
        cursor: pointer;
    }
    .wh-cmd-copy:hover {
        color: var(--color-accent);
        border-color: color-mix(in srgb, var(--color-accent) 50%, var(--color-border));
    }
    .wh-cmd-copy :global(.wh-cmd-ico) {
        width: 14px;
        height: 14px;
    }
    .wh-row-ctl {
        flex: none;
        padding-top: 2px;
    }
</style>
