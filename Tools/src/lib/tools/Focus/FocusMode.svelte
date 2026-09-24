<script lang="ts">
    /*
      Focus Mode — start a session that nudges you off distracting apps. The
      session + enforcement live in $lib/stores/focusMode (persist across
      navigation); this is the setup + status surface. Non-destructive: nudge
      (notification) and minimize only. There's always a Stop, and the session
      auto-ends at its deadline.
    */
    import { onMount, onDestroy } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { Button, ToolPage } from '$lib/ui';
    import {
        focusConfig,
        focusSession,
        focusSnoozes,
        focusLastSummary,
        startFocus,
        endFocus,
        setFocusMode,
        addToBlocklist,
        removeFromBlocklist,
        addToAllowlist,
        removeFromAllowlist,
        addBlockedSite,
        removeBlockedSite,
        isBlockedApp,
        setFocusAction,
        setFocusDuration,
        snoozeApp,
        normalizeApp,
    } from '$lib/stores/focusMode';
    import {
        Target,
        Plus,
        X,
        Play,
        Square,
        BellRing,
        Minimize2,
        LogOut,
        Coffee,
        ShieldAlert,
        Ban,
        ShieldCheck,
        Clock,
    } from '@lucide/svelte';

    let manualName = $state('');
    let siteName = $state('');
    let currentApp = $state('');
    let now = $state(Date.now());

    let appPollId: ReturnType<typeof setInterval> | null = null;
    let clockId: ReturnType<typeof setInterval> | null = null;
    onMount(() => {
        const pollApp = async () => {
            try {
                currentApp = await invoke<string>('voice_get_foreground_app');
            } catch {
                currentApp = '';
            }
        };
        void pollApp();
        appPollId = setInterval(() => void pollApp(), 2000);
        clockId = setInterval(() => (now = Date.now()), 1000);
    });
    onDestroy(() => {
        if (appPollId) clearInterval(appPollId);
        if (clockId) clearInterval(clockId);
    });

    let active = $derived($focusSession.active);
    let remainingMs = $derived(Math.max(0, $focusSession.endsAt - now));
    let remaining = $derived(
        `${Math.floor(remainingMs / 60000)
            .toString()
            .padStart(2, '0')}:${Math.floor((remainingMs % 60000) / 1000)
            .toString()
            .padStart(2, '0')}`,
    );

    let mode = $derived($focusConfig.mode);
    let activeList = $derived(mode === 'allowlist' ? $focusConfig.allowlist : $focusConfig.blocklist);
    let blockedSites = $derived($focusConfig.blockedSites);
    let canStart = $derived(
        mode === 'allowlist'
            ? $focusConfig.allowlist.length > 0
            : $focusConfig.blocklist.length > 0 || blockedSites.length > 0,
    );

    let currentNorm = $derived(normalizeApp(currentApp));
    let currentBlocked = $derived(!!currentApp && isBlockedApp(currentNorm, $focusConfig));
    let currentOnList = $derived(!!currentApp && activeList.includes(currentNorm));
    let currentSnoozed = $derived(($focusSnoozes[currentNorm] ?? 0) > now);
    function snoozeMins(until: number): number {
        return Math.max(0, Math.ceil((until - now) / 60000));
    }

    let listLabel = $derived(mode === 'allowlist' ? 'Allow list' : 'Blocklist');
    let addPlaceholder = $derived(
        mode === 'allowlist'
            ? 'allow an app — e.g. code, figma, notion'
            : 'block an app — e.g. discord, steam, chrome',
    );
    let emptyText = $derived(
        mode === 'allowlist'
            ? 'No apps allowed yet. Add the ones you need — everything else gets blocked.'
            : 'No apps blocked yet. Add the ones that pull your attention.',
    );

    function addToActiveList(name: string) {
        if (mode === 'allowlist') addToAllowlist(name);
        else addToBlocklist(name);
    }
    function removeFromActiveList(name: string) {
        if (mode === 'allowlist') removeFromAllowlist(name);
        else removeFromBlocklist(name);
    }
    function addManual() {
        if (!manualName.trim()) return;
        addToActiveList(manualName);
        manualName = '';
    }
    function addSite() {
        if (!siteName.trim()) return;
        addBlockedSite(siteName);
        siteName = '';
    }
    function fmtSecs(s: number): string {
        if (s < 60) return `${s}s`;
        const m = Math.floor(s / 60);
        const r = s % 60;
        return r ? `${m}m ${r}s` : `${m}m`;
    }
</script>

<ToolPage
    icon={Target}
    iconTint="#8b5cf6"
    title="Focus Mode"
    description="Protect a quiet block of time with local, non-destructive nudges for distracting apps and sites."
    width="medium"
    fill={false}
>
<div class="fm-root">

    {#if active}
        <section class="fm-live">
            <div class="fm-live-badge">FOCUS ON</div>
            <div class="fm-live-time">{remaining}</div>
            <div class="fm-live-sub">
                {#if mode === 'allowlist'}
                    Allowing only {activeList.length} app{activeList.length === 1 ? '' : 's'} · everything
                    else blocked
                {:else}
                    Blocking {activeList.length} app{activeList.length === 1 ? '' : 's'}
                {/if}
                {#if blockedSites.length}+ {blockedSites.length} site{blockedSites.length === 1 ? '' : 's'}{/if}
                · {$focusConfig.action === 'close'
                    ? 'nudge + close'
                    : $focusConfig.action === 'minimize'
                      ? 'nudge + minimize'
                      : 'nudge only'}
            </div>
            <Button variant="danger" icon={Square} onclick={() => endFocus('Focus session ended.')}>
                Stop focus
            </Button>
        </section>

        {#if currentApp && currentBlocked && !currentSnoozed}
            <div class="fm-callout">
                <ShieldAlert class="fm-callout-ico" aria-hidden="true" />
                <span><strong>{currentApp}</strong> is blocked right now.</span>
                <Button
                    variant="secondary"
                    size="sm"
                    icon={Coffee}
                    onclick={() => snoozeApp(currentApp)}
                >
                    Snooze 3 min
                </Button>
            </div>
        {/if}

        <span class="fm-label fm-active-label">{mode === 'allowlist' ? 'Allowed apps' : 'Blocklist'}</span>
        <div class="fm-active-list">
            {#each activeList as app (app)}
                {@const sUntil = $focusSnoozes[app] ?? 0}
                {@const snoozed = sUntil > now}
                <div class="fm-active-item">
                    <span class="fm-active-name">{app}</span>
                    {#if mode === 'blocklist'}
                        {#if snoozed}
                            <span class="fm-snoozed">snoozed {snoozeMins(sUntil)}m</span>
                        {:else}
                            <button
                                type="button"
                                class="fm-snooze-btn"
                                onclick={() => snoozeApp(app)}
                            >
                                <Coffee class="fm-snooze-ico" />
                                Snooze 3m
                            </button>
                        {/if}
                    {:else}
                        <span class="fm-snoozed">allowed</span>
                    {/if}
                </div>
            {/each}
        </div>
    {:else}
        {#if $focusLastSummary}
            {@const sum = $focusLastSummary}
            {@const total = sum.apps.reduce((acc, x) => acc + x.seconds, 0) || 1}
            <section class="fm-summary">
                <div class="fm-summary-head">
                    <Clock class="fm-summary-ico" />
                    <span>Last session · {sum.durationMin} min</span>
                    <button type="button" class="fm-summary-x" title="Dismiss" onclick={() => focusLastSummary.set(null)}>
                        <X class="fm-chip-x" />
                    </button>
                </div>
                {#if sum.apps.length}
                    <div class="fm-summary-bars">
                        {#each sum.apps.slice(0, 6) as a (a.app)}
                            <div class="fm-bar-row">
                                <span class="fm-bar-name">{a.app}</span>
                                <span class="fm-bar-track"><span class="fm-bar-fill" style="width:{Math.round((a.seconds / total) * 100)}%"></span></span>
                                <span class="fm-bar-time">{fmtSecs(a.seconds)}</span>
                            </div>
                        {/each}
                    </div>
                {/if}
                <p class="fm-summary-foot">
                    {#if sum.blockedSeconds > 0}
                        {fmtSecs(sum.blockedSeconds)} on blocked apps/sites — the nudges did their job.
                    {:else}
                        You stayed clear of everything blocked. Nice.
                    {/if}
                </p>
            </section>
        {/if}
        <section class="fm-setup">
            <div class="fm-row">
                <label class="fm-field">
                    <span>Session length (min)</span>
                    <input
                        type="number"
                        min="1"
                        max="240"
                        value={$focusConfig.durationMin}
                        onchange={(e) => setFocusDuration(Number(e.currentTarget.value))}
                    />
                </label>
            </div>

            <div class="fm-mode">
                <span class="fm-label">Focus rule</span>
                <div class="fm-mode-opts">
                    <button
                        type="button"
                        class="fm-opt"
                        class:is-on={mode === 'blocklist'}
                        aria-pressed={mode === 'blocklist'}
                        onclick={() => setFocusMode('blocklist')}
                    >
                        <Ban class="fm-opt-ico" />
                        <span class="fm-opt-name">Block list</span>
                        <span class="fm-opt-desc">
                            Block the apps you list — everything else stays free.
                        </span>
                    </button>
                    <button
                        type="button"
                        class="fm-opt"
                        class:is-on={mode === 'allowlist'}
                        aria-pressed={mode === 'allowlist'}
                        onclick={() => setFocusMode('allowlist')}
                    >
                        <ShieldCheck class="fm-opt-ico" />
                        <span class="fm-opt-name">Allow list</span>
                        <span class="fm-opt-desc">
                            Allow only the apps you list — block everything else automatically.
                        </span>
                    </button>
                </div>
            </div>

            <div class="fm-action">
                <span class="fm-label">
                    {mode === 'allowlist'
                        ? 'When you open a non-allowed app'
                        : 'When you open a blocked app'}
                </span>
                <div class="fm-action-opts">
                    <button
                        type="button"
                        class="fm-opt"
                        class:is-on={$focusConfig.action === 'nudge'}
                        aria-pressed={$focusConfig.action === 'nudge'}
                        onclick={() => setFocusAction('nudge')}
                    >
                        <BellRing class="fm-opt-ico" />
                        <span class="fm-opt-name">Nudge only</span>
                        <span class="fm-opt-desc">A notification — never touches the window.</span>
                    </button>
                    <button
                        type="button"
                        class="fm-opt"
                        class:is-on={$focusConfig.action === 'minimize'}
                        aria-pressed={$focusConfig.action === 'minimize'}
                        onclick={() => setFocusAction('minimize')}
                    >
                        <Minimize2 class="fm-opt-ico" />
                        <span class="fm-opt-name">Nudge + minimize</span>
                        <span class="fm-opt-desc">Pushes it out of sight. No data loss.</span>
                    </button>
                    <button
                        type="button"
                        class="fm-opt"
                        class:is-on={$focusConfig.action === 'close'}
                        aria-pressed={$focusConfig.action === 'close'}
                        onclick={() => setFocusAction('close')}
                    >
                        <LogOut class="fm-opt-ico" />
                        <span class="fm-opt-name">Nudge + close</span>
                        <span class="fm-opt-desc">Gracefully closes it — the app can still prompt to save.</span>
                    </button>
                </div>
                <p class="fm-suspend-note">
                    Every action is non-destructive — even <strong>Close</strong>.
                </p>
            </div>

            <div class="fm-block">
                <span class="fm-label">{listLabel}</span>
                <div class="fm-current">
                    {#if currentApp}
                        <span class="fm-current-label">
                            Currently focused: <strong>{currentApp}</strong>
                        </span>
                        {#if currentOnList}
                            <span class="fm-current-tag">
                                {mode === 'allowlist' ? 'allowed' : 'on blocklist'}
                            </span>
                        {:else}
                            <Button
                                variant="secondary"
                                size="sm"
                                icon={Plus}
                                onclick={() => addToActiveList(currentApp)}
                            >
                                {mode === 'allowlist' ? 'Allow this' : 'Block this'}
                            </Button>
                        {/if}
                    {/if}
                </div>
                <div class="fm-add">
                    <input
                        class="fm-input"
                        bind:value={manualName}
                        onkeydown={(e) => {
                            if (e.key === 'Enter') {
                                e.preventDefault();
                                addManual();
                            }
                        }}
                        placeholder={addPlaceholder}
                        spellcheck="false"
                    />
                    <Button variant="secondary" icon={Plus} onclick={addManual}>Add</Button>
                </div>

                {#if activeList.length === 0}
                    <p class="fm-empty">{emptyText}</p>
                {:else}
                    <div class="fm-chips">
                        {#each activeList as app (app)}
                            <span class="fm-chip">
                                {app}
                                <button
                                    type="button"
                                    title="Remove"
                                    onclick={() => removeFromActiveList(app)}
                                >
                                    <X class="fm-chip-x" />
                                </button>
                            </span>
                        {/each}
                    </div>
                {/if}
            </div>

            <div class="fm-block">
                <span class="fm-label">Blocked websites</span>
                <p class="fm-sites-hint">
                    Blocks a browser tab when its title contains the word — nudge only, so your other
                    tabs stay open.
                </p>
                <div class="fm-add">
                    <input
                        class="fm-input"
                        bind:value={siteName}
                        onkeydown={(e) => {
                            if (e.key === 'Enter') {
                                e.preventDefault();
                                addSite();
                            }
                        }}
                        placeholder="block a site — e.g. youtube, reddit, x.com"
                        spellcheck="false"
                    />
                    <Button variant="secondary" icon={Plus} onclick={addSite}>Add</Button>
                </div>
                {#if blockedSites.length === 0}
                    <p class="fm-empty">No sites blocked. Add keywords like youtube or reddit.</p>
                {:else}
                    <div class="fm-chips">
                        {#each blockedSites as site (site)}
                            <span class="fm-chip">
                                {site}
                                <button type="button" title="Remove" onclick={() => removeBlockedSite(site)}>
                                    <X class="fm-chip-x" />
                                </button>
                            </span>
                        {/each}
                    </div>
                {/if}
            </div>

            <div class="fm-start">
                <Button
                    variant="primary"
                    icon={Play}
                    disabled={!canStart}
                    onclick={startFocus}
                >
                    Start focus session
                </Button>
            </div>
        </section>

    {/if}
</div>
</ToolPage>

<style>
    .fm-root {
        max-width: 960px;
        margin: 0;
        display: flex;
        flex-direction: column;
        gap: 18px;
    }

    .fm-live {
        position: relative;
        display: grid;
        grid-template-columns: minmax(0, 1fr) auto;
        gap: 5px 24px;
        align-items: center;
        margin-top: 2px;
        padding: 18px 0 20px 16px;
        border-top: 1px solid var(--color-border);
        border-bottom: 1px solid var(--color-border);
    }

    .fm-live::before,
    .fm-opt.is-on::before {
        position: absolute;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }

    .fm-live::before {
        top: 18px;
        bottom: 20px;
        left: 0;
    }

    .fm-live-badge {
        display: inline-flex;
        align-items: center;
        gap: 7px;
        font-size: 11px;
        font-weight: 700;
        letter-spacing: 0.09em;
        color: var(--color-accent);
    }

    .fm-live-badge::before {
        width: 6px;
        height: 6px;
        border-radius: 999px;
        background: var(--color-accent);
        box-shadow: 0 0 0 4px color-mix(in srgb, var(--color-accent) 14%, transparent);
        content: '';
    }

    .fm-live-time {
        margin: 2px 0;
        font-size: clamp(40px, 7vw, 56px);
        font-weight: 700;
        line-height: 1;
        letter-spacing: -0.05em;
        font-variant-numeric: tabular-nums;
        color: var(--color-text);
    }

    .fm-live-sub {
        max-width: 68ch;
        margin: 0;
        font-size: 13px;
        line-height: 1.45;
        color: var(--color-text-secondary);
    }

    .fm-live :global(.btn) {
        grid-column: 2;
        grid-row: 1 / span 3;
        align-self: center;
    }

    .fm-callout {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 11px 13px;
        font-size: 13px;
        color: var(--color-text);
        background: var(--color-panel-2);
        border-left: 2px solid var(--color-accent);
        border-radius: var(--radius-control, 8px);
    }

    .fm-callout > span {
        flex: 1;
        min-width: 0;
    }

    .fm-callout :global(.fm-callout-ico) {
        flex: none;
        width: 16px;
        height: 16px;
        color: var(--color-accent);
    }

    .fm-label {
        display: block;
        margin-bottom: 9px;
        font-size: 11px;
        font-weight: 700;
        letter-spacing: 0.065em;
        text-transform: uppercase;
        color: var(--color-muted);
    }

    .fm-active-label {
        margin: 2px 0 0;
    }

    .fm-active-list {
        display: flex;
        flex-direction: column;
        border-top: 1px solid var(--color-border);
    }

    .fm-active-item {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 11px 4px;
        border-bottom: 1px solid var(--color-border);
    }

    .fm-active-name {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
    }

    .fm-snoozed {
        font-size: 11.5px;
        font-weight: 600;
        color: var(--color-muted);
    }

    .fm-snooze-btn {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        min-height: 28px;
        padding: 4px 10px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        color: var(--color-text-secondary);
        background: var(--color-panel-2);
        font-size: 12px;
        font-weight: 600;
        cursor: pointer;
    }

    .fm-snooze-btn:hover {
        border-color: color-mix(in srgb, var(--color-accent) 42%, var(--color-border));
        color: var(--color-text);
    }

    .fm-snooze-btn :global(.fm-snooze-ico),
    .fm-chip :global(.fm-chip-x) {
        width: 13px;
        height: 13px;
    }

    .fm-setup {
        display: flex;
        flex-direction: column;
        margin-top: 0;
        border-top: 1px solid var(--color-border);
    }

    .fm-row,
    .fm-mode,
    .fm-action,
    .fm-block,
    .fm-start {
        padding: 20px 0;
        border-bottom: 1px solid var(--color-border);
    }

    .fm-row {
        display: flex;
        align-items: center;
    }

    .fm-field {
        display: grid;
        grid-template-columns: minmax(0, 1fr) 112px;
        align-items: center;
        gap: 14px;
        width: min(100%, 360px);
        padding: 7px 8px 7px 13px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }

    .fm-field span {
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
    }

    .fm-field input,
    .fm-input {
        height: 36px;
        padding: 0 11px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        color: var(--color-text);
        background: var(--color-panel);
        font: inherit;
        font-size: 13px;
    }

    .fm-field input {
        width: 100%;
    }

    .fm-field:focus-within {
        border-color: color-mix(in srgb, var(--color-accent) 58%, var(--color-border));
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 14%, transparent);
    }

    .fm-field input:focus,
    .fm-input:focus {
        outline: none;
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
    }

    .fm-mode-opts {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: 8px;
    }

    .fm-action-opts {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr));
        gap: 8px;
    }

    .fm-opt {
        position: relative;
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        min-height: 108px;
        gap: 5px;
        padding: 14px 15px 14px 18px;
        overflow: hidden;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 10px);
        color: var(--color-text);
        background: var(--color-panel);
        text-align: left;
        cursor: pointer;
        transition:
            background var(--dur-micro, 150ms) var(--ease-out),
            border-color var(--dur-micro, 150ms) var(--ease-out);
    }

    .fm-opt:hover,
    .fm-opt.is-on {
        background: var(--color-panel-2);
    }

    .fm-opt:hover {
        border-color: color-mix(in srgb, var(--color-accent) 38%, var(--color-border));
    }

    .fm-opt.is-on {
        border-color: var(--color-border);
    }

    .fm-opt.is-on::before {
        top: 12px;
        bottom: 12px;
        left: 0;
    }

    .fm-opt :global(.fm-opt-ico) {
        width: 18px;
        height: 18px;
        color: var(--color-text-secondary);
    }

    .fm-opt.is-on :global(.fm-opt-ico) {
        color: var(--color-accent);
    }

    .fm-opt-name {
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
    }

    .fm-opt-desc,
    .fm-suspend-note,
    .fm-sites-hint {
        font-size: 12px;
        line-height: 1.45;
        color: var(--color-text-secondary);
    }

    .fm-suspend-note {
        margin: 10px 0 0;
        color: var(--color-muted);
    }

    .fm-current {
        display: flex;
        align-items: center;
        gap: 10px;
        min-height: 34px;
        margin-bottom: 10px;
        padding: 0 2px;
    }

    .fm-current-label {
        font-size: 12.5px;
        color: var(--color-text-secondary);
    }

    .fm-current-tag {
        padding: 0;
        border: 0;
        color: var(--color-accent);
        background: transparent;
        font-size: 11px;
        font-weight: 600;
    }

    .fm-add {
        display: flex;
        gap: 8px;
    }

    .fm-add .fm-input {
        flex: 1;
        min-width: 0;
        min-height: 38px;
    }

    .fm-empty {
        margin: 10px 0 0;
        padding: 10px 0 1px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 72%, transparent);
        font-size: 13px;
        color: var(--color-text-secondary);
    }

    .fm-chips {
        display: flex;
        flex-wrap: wrap;
        gap: 8px;
        margin-top: 12px;
    }

    .fm-chip {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        min-height: 30px;
        padding: 4px 6px 4px 11px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        color: var(--color-text);
        background: var(--color-panel-2);
        font-size: 12.5px;
    }

    .fm-chip button,
    .fm-summary-x {
        display: grid;
        place-items: center;
        border: 0;
        color: var(--color-muted);
        background: transparent;
        cursor: pointer;
    }

    .fm-chip button {
        width: 18px;
        height: 18px;
        border-radius: 999px;
    }

    .fm-chip button:hover {
        color: var(--color-error);
        background: color-mix(in srgb, var(--color-error) 12%, transparent);
    }

    .fm-sites-hint {
        max-width: 74ch;
        margin: -1px 0 10px;
        color: var(--color-muted);
    }

    .fm-start {
        display: flex;
        justify-content: flex-end;
        padding-bottom: 0;
        border-bottom: 0;
    }

    .fm-summary {
        margin-top: 2px;
        padding: 17px 0 18px;
        border-top: 1px solid var(--color-border);
        border-bottom: 1px solid var(--color-border);
    }

    .fm-summary-head {
        display: flex;
        align-items: center;
        gap: 8px;
        padding-left: 2px;
        font-size: 13px;
        font-weight: 700;
        color: var(--color-text);
    }

    .fm-summary-head :global(.fm-summary-ico) {
        width: 16px;
        height: 16px;
        color: var(--color-accent);
    }

    .fm-summary-x {
        width: 24px;
        height: 24px;
        margin-left: auto;
        border: 1px solid transparent;
        border-radius: var(--radius-control, 8px);
    }

    .fm-summary-x:hover {
        color: var(--color-text);
        background: var(--color-panel-2);
    }

    .fm-summary-bars {
        display: flex;
        flex-direction: column;
        gap: 8px;
        margin-top: 12px;
        padding-top: 12px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 72%, transparent);
    }

    .fm-bar-row {
        display: flex;
        align-items: center;
        gap: 10px;
        font-size: 12.5px;
    }

    .fm-bar-name {
        flex: none;
        width: 110px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--color-text);
    }

    .fm-bar-track {
        flex: 1;
        height: 8px;
        overflow: hidden;
        border-radius: 999px;
        background: var(--color-panel-2);
    }

    .fm-bar-fill {
        display: block;
        height: 100%;
        border-radius: 999px;
        background: var(--color-accent);
    }

    .fm-bar-time {
        flex: none;
        width: 54px;
        text-align: right;
        font-variant-numeric: tabular-nums;
        color: var(--color-text-secondary);
    }

    .fm-summary-foot {
        margin: 14px 0 0;
        font-size: 12.5px;
        line-height: 1.48;
        color: var(--color-text-secondary);
    }

    .fm-opt:focus-visible,
    .fm-snooze-btn:focus-visible,
    .fm-chip button:focus-visible,
    .fm-summary-x:focus-visible {
        outline: none;
        border-color: var(--color-accent);
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 22%, transparent);
    }

    @media (max-width: 720px) {
        .fm-action-opts {
            grid-template-columns: 1fr;
        }
    }

    @media (max-width: 640px) {
        .fm-live {
            grid-template-columns: 1fr;
            gap: 7px;
        }

        .fm-live :global(.btn) {
            grid-column: auto;
            grid-row: auto;
            justify-self: start;
            margin-top: 6px;
        }

        .fm-mode-opts {
            grid-template-columns: 1fr;
        }

        .fm-field {
            width: 100%;
        }

        .fm-callout,
        .fm-current {
            align-items: flex-start;
            flex-wrap: wrap;
        }

        .fm-start,
        .fm-start :global(.btn) {
            width: 100%;
        }
    }

    @media (prefers-reduced-motion: reduce) {
        .fm-opt {
            transition: none;
        }
    }
</style>
