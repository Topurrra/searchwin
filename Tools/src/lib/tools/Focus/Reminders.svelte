<script lang="ts">
    /*
      Reminders — add local one-off reminders that fire a system notification
      when due. The scheduler lives in $lib/stores/reminders (runs app-wide);
      this is just the management surface. Accepts natural-language times
      ("in 20 min", "at 3pm", "tomorrow 9am") or an exact picker.
    */
    import { onMount } from 'svelte';
    import { Button, ToolPage } from '$lib/ui';
    import {
        reminders,
        addReminder,
        removeReminder,
        clearFired,
        parseReminderTime,
        startReminderScheduler,
    } from '$lib/stores/reminders';
    import { Bell, Trash2, Plus } from '@lucide/svelte';
    onMount(startReminderScheduler);

    let text = $state('');
    let whenText = $state('');
    let exact = $state(''); // datetime-local value

    let parsedMs = $derived(parseReminderTime(whenText));
    let exactMs = $derived(exact ? new Date(exact).getTime() : null);
    let dueMs = $derived<number | null>(exactMs ?? parsedMs);
    let dueValid = $derived(dueMs !== null && dueMs > Date.now());
    let canAdd = $derived(text.trim().length > 0 && dueValid);

    function add() {
        if (!canAdd || dueMs === null) return;
        addReminder(text, dueMs);
        text = '';
        whenText = '';
        exact = '';
    }

    function setWhen(phrase: string) {
        whenText = phrase;
        exact = '';
    }

    function fmtAbs(ms: number): string {
        const d = new Date(ms);
        const now = new Date();
        const tomorrow = new Date(now);
        tomorrow.setDate(now.getDate() + 1);
        const time = d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
        if (d.toDateString() === now.toDateString()) return `Today ${time}`;
        if (d.toDateString() === tomorrow.toDateString()) return `Tomorrow ${time}`;
        return `${d.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' })} ${time}`;
    }

    let upcoming = $derived(
        $reminders.filter((r) => !r.fired).sort((a, b) => a.dueMs - b.dueMs),
    );
    let past = $derived($reminders.filter((r) => r.fired).sort((a, b) => b.dueMs - a.dueMs));

    function onKeydown(e: KeyboardEvent) {
        if (e.key === 'Enter' && canAdd) {
            e.preventDefault();
            add();
        }
    }
</script>

<ToolPage
    icon={Bell}
    iconTint="#8b5cf6"
    title="Reminders"
    description="Capture a thought, set a local time, and KeepItLocal will notify you when it is due."
    width="medium"
    fill={false}
>
<div class="rem-root">

    <div class="rem-add">
        <div class="rem-add-head">
            <div>
                <span class="rem-kicker">NEW REMINDER</span>
                <strong>What needs your attention?</strong>
            </div>
            <span class="rem-add-note">Runs locally</span>
        </div>
        <input
            class="rem-input rem-text"
            bind:value={text}
            onkeydown={onKeydown}
            placeholder="Remind me to…"
            spellcheck="false"
        />
        <div class="rem-when-row">
            <input
                class="rem-input rem-when"
                bind:value={whenText}
                onkeydown={onKeydown}
                placeholder="when — e.g. in 20 min, at 3pm, tomorrow 9am"
                spellcheck="false"
                disabled={!!exact}
            />
            <input class="rem-input rem-exact" type="datetime-local" bind:value={exact} />
        </div>
        <div class="rem-quick">
            <span class="rem-quick-label">Quick schedule</span>
            <button type="button" onclick={() => setWhen('in 15 minutes')}>+15 min</button>
            <button type="button" onclick={() => setWhen('in 1 hour')}>+1 hour</button>
            <button type="button" onclick={() => setWhen('in 3 hours')}>+3 hours</button>
            <button type="button" onclick={() => setWhen('tomorrow 9am')}>Tomorrow 9am</button>
            <span class="rem-preview" class:ok={dueValid} class:bad={!!whenText && !dueValid && !exact} aria-live="polite">
                {#if dueValid && dueMs !== null}
                    → {fmtAbs(dueMs)}
                {:else if whenText && !exact}
                    couldn't read that time
                {/if}
            </span>
            <Button variant="primary" icon={Plus} disabled={!canAdd} onclick={add}>Add</Button>
        </div>
    </div>

    {#if upcoming.length === 0 && past.length === 0}
        <div class="rem-empty">
            <Bell size={18} aria-hidden="true" />
            <div>
                <strong>Your day is clear</strong>
                <p>Add a reminder above and it will stay here until it fires.</p>
            </div>
        </div>
    {:else}
        {#if upcoming.length}
            <div class="rem-section-label">Upcoming</div>
            <div class="rem-list">
                {#each upcoming as r (r.id)}
                    <div class="rem-item">
                        <span class="rem-status" aria-hidden="true"></span>
                        <div class="rem-item-text">
                            <div class="rem-item-title">{r.text}</div>
                            <div class="rem-item-due">{fmtAbs(r.dueMs)}</div>
                        </div>
                        <button
                            type="button"
                            class="rem-del"
                            title="Delete reminder"
                            onclick={() => removeReminder(r.id)}
                        >
                            <Trash2 class="rem-del-ico" />
                        </button>
                    </div>
                {/each}
            </div>
        {/if}

        {#if past.length}
            <div class="rem-section-label rem-section-past">
                <span>Past</span>
                <button type="button" class="rem-clear" onclick={clearFired}>Clear</button>
            </div>
            <div class="rem-list">
                {#each past as r (r.id)}
                    <div class="rem-item is-past">
                        <span class="rem-status" aria-hidden="true"></span>
                        <div class="rem-item-text">
                            <div class="rem-item-title">{r.text}</div>
                            <div class="rem-item-due">{fmtAbs(r.dueMs)}</div>
                        </div>
                        <button
                            type="button"
                            class="rem-del"
                            title="Delete reminder"
                            onclick={() => removeReminder(r.id)}
                        >
                            <Trash2 class="rem-del-ico" />
                        </button>
                    </div>
                {/each}
            </div>
        {/if}
    {/if}
</div>
</ToolPage>

<style>
    .rem-root {
        max-width: 760px;
        margin: 0;
        display: flex;
        flex-direction: column;
    }

    .rem-add {
        margin: 0;
        padding: 2px 0 24px;
        display: flex;
        flex-direction: column;
        gap: 12px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }

    .rem-add-head {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 16px;
    }

    .rem-add-head > div {
        display: flex;
        flex-direction: column;
        gap: 3px;
    }

    .rem-kicker,
    .rem-quick-label {
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.08em;
        text-transform: uppercase;
    }

    .rem-add-head strong {
        color: var(--color-text);
        font-size: 14px;
        font-weight: 600;
    }

    .rem-add-note {
        flex: none;
        padding: 4px 8px;
        border: 1px solid var(--color-border);
        border-radius: 999px;
        color: var(--color-text-secondary);
        font-size: 11px;
        line-height: 1;
    }
    .rem-input {
        box-sizing: border-box;
        width: 100%;
        height: 42px;
        padding: 0 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-family: inherit;
        font-size: 13px;
    }

    .rem-text {
        height: 44px;
        font-size: 14px;
    }

    .rem-input:focus-visible {
        outline: none;
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 16%, transparent);
    }

    .rem-input:disabled {
        cursor: not-allowed;
        opacity: 0.56;
    }

    .rem-when-row {
        display: grid;
        grid-template-columns: minmax(0, 1fr) 206px;
        gap: 8px;
    }

    .rem-when {
        min-width: 0;
    }

    .rem-exact {
        color-scheme: dark;
    }

    .rem-quick {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 7px;
        padding-top: 2px;
    }

    .rem-quick-label {
        margin-right: 3px;
    }

    .rem-quick > button {
        min-height: 30px;
        padding: 0 10px;
        border: 1px solid var(--color-border);
        border-radius: 999px;
        background: transparent;
        color: var(--color-text-secondary);
        cursor: pointer;
        font-family: inherit;
        font-size: 12px;
        font-weight: 600;
    }

    .rem-quick > button:hover {
        border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
        color: var(--color-text);
    }

    .rem-quick > button:focus-visible,
    .rem-clear:focus-visible,
    .rem-del:focus-visible {
        outline: 2px solid color-mix(in srgb, var(--color-accent) 72%, transparent);
        outline-offset: 2px;
    }

    .rem-preview {
        display: inline-flex;
        align-items: center;
        min-height: 30px;
        margin-left: auto;
        color: var(--color-text-secondary);
        font-size: 12px;
    }

    .rem-preview.ok {
        color: var(--color-accent);
        font-weight: 600;
    }

    .rem-preview.bad {
        color: var(--color-error, #b5352c);
    }

    .rem-empty {
        display: flex;
        align-items: center;
        gap: 12px;
        margin: 28px 0 0;
        padding: 20px 4px;
        border-top: 1px solid var(--color-divider, var(--color-border));
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        color: var(--color-text-secondary);
    }

    .rem-empty :global(svg) {
        flex: none;
        color: var(--color-muted);
    }

    .rem-empty > div {
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    .rem-empty strong {
        color: var(--color-text);
        font-size: 13px;
        font-weight: 600;
    }

    .rem-empty p {
        margin: 0;
        font-size: 12px;
    }
    .rem-add + .rem-section-label {
        margin-top: 26px;
    }

    .rem-section-label {
        display: flex;
        align-items: center;
        gap: 8px;
        margin: 0 2px 8px;
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.08em;
        text-transform: uppercase;
    }

    .rem-list + .rem-section-label {
        margin-top: 28px;
    }

    .rem-section-past {
        justify-content: space-between;
    }
    .rem-clear {
        padding: 2px 0;
        border: 0;
        background: transparent;
        color: var(--color-text-secondary);
        cursor: pointer;
        font-family: inherit;
        font-size: 11px;
        font-weight: 600;
        letter-spacing: 0;
        text-transform: none;
    }

    .rem-clear:hover {
        color: var(--color-text);
    }
    .rem-list {
        display: flex;
        flex-direction: column;
        border-top: 1px solid var(--color-divider, var(--color-border));
    }

    .rem-item {
        display: grid;
        grid-template-columns: 10px minmax(0, 1fr) auto;
        align-items: center;
        gap: 12px;
        padding: 13px 6px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }

    .rem-item.is-past {
        opacity: 0.56;
    }

    .rem-status {
        width: 8px;
        height: 8px;
        border-radius: 999px;
        background: var(--color-accent);
        box-shadow: 0 0 0 4px color-mix(in srgb, var(--color-accent) 10%, transparent);
    }

    .rem-item.is-past .rem-status {
        background: var(--color-muted);
        box-shadow: none;
    }
    .rem-item-text {
        min-width: 0;
    }

    .rem-item-title {
        overflow: hidden;
        color: var(--color-text);
        font-size: 13.5px;
        font-weight: 600;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .rem-item-due {
        margin-top: 3px;
        color: var(--color-text-secondary);
        font-size: 12px;
    }

    .rem-del {
        display: grid;
        flex: none;
        width: 30px;
        height: 30px;
        place-items: center;
        border: 1px solid transparent;
        border-radius: var(--radius-control, 8px);
        background: transparent;
        color: var(--color-muted);
        cursor: pointer;
    }

    .rem-del:hover {
        border-color: color-mix(in srgb, var(--color-error, #b5352c) 28%, var(--color-border));
        color: var(--color-error, #b5352c);
    }

    .rem-del :global(.rem-del-ico) {
        width: 15px;
        height: 15px;
    }


    @media (max-width: 640px) {
        .rem-add-head {
            align-items: flex-start;
            flex-direction: column;
            gap: 8px;
        }

        .rem-when-row {
            grid-template-columns: 1fr;
        }

        .rem-quick-label,
        .rem-preview {
            width: 100%;
        }

        .rem-preview {
            margin-left: 0;
        }
    }
</style>
