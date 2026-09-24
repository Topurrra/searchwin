<script lang="ts">
    import { onMount } from 'svelte';
    import { Lock, Loader2, Mic, Radio } from '@lucide/svelte';
    import {
        fileSearchStatus,
        initFileSearchStore,
        refreshFileSearchStatus,
    } from '$lib/stores/fileSearch';
    import { busyTasks, primaryBusyTask } from '$lib/stores/globalBusy';
    import { voiceActive } from '$lib/stores/voiceActivity';
    import { pushToTalk } from '$lib/stores/pushToTalk';
    import { commandMode, requestToggleCommandMode } from '$lib/stores/commandMode';
    import { _ } from 'svelte-i18n';

    let busy = $derived($busyTasks.length > 0);

    let busyLabel = $derived.by(() => {
        const primary = $primaryBusyTask;
        if (!primary) return '';
        const others = $busyTasks.length - 1;
        return others > 0 ? `${primary.label} · +${others} more` : primary.label;
    });

    let searchReady = $derived(Boolean(
        $fileSearchStatus?.initialized ||
        ($fileSearchStatus?.indexedFiles ?? 0) > 0 ||
        $fileSearchStatus?.lastIndexedAtMs
    ));
    let searchWatching = $derived(Boolean(
        $fileSearchStatus?.watching ||
        $fileSearchStatus?.diagnostics?.watcherWorker === 'running' ||
        $fileSearchStatus?.diagnostics?.watcherWorker === 'starting'
    ));
    let searchWatcherPaused = $derived(Boolean($fileSearchStatus?.watcherPaused));
    let searchWatcherMessage = $derived($fileSearchStatus?.diagnostics?.lastWorkerMessage ?? '');
    let searchWatcherFoldersOnly = $derived(Boolean(
        searchReady &&
        !searchWatching &&
        !searchWatcherPaused &&
        searchWatcherMessage.includes('explicit folders')
    ));

    let watcherWorking = $derived(searchWatching);
    let searchWatcherLabel = $derived(
        searchWatching
            ? $_('statusBar.watcherOn')
            : searchWatcherPaused
              ? $_('statusBar.watcherPaused')
              : searchWatcherFoldersOnly
                ? $_('statusBar.watcherFoldersOnly')
                : $_('statusBar.watcherOff')
    );
    let searchIndexing = $derived(Boolean($fileSearchStatus?.indexing));
    let searchError = $derived(searchIndexing || searchReady ? null : ($fileSearchStatus?.lastError ?? null));
    let searchIndexedFiles = $derived($fileSearchStatus?.indexedFiles ?? 0);

    // Per-pack subscriptions retired — every tool that wants to advertise
    // a busy state now calls `reportBusy()` from `$lib/stores/globalBusy`,
    // and the StatusBar reads `$busyTasks` directly above. No dynamic
    // imports + no per-pack toggle plumbing needed here anymore.

    onMount(() => {
        void initFileSearchStore();
        void refreshFileSearchStatus();
    });
</script>

<footer class="h-7 border-t border-border bg-panel flex items-center justify-between px-3 text-[11px] shrink-0">
    <div class="flex items-center gap-3">
        <span class="trust-pill">
            <Lock class="w-3 h-3" />
            <span>{$_('statusBar.trustPill')}</span>
        </span>
        {#if busy}
            <div class="flex items-center gap-1 text-text-secondary">
                <Loader2 class="w-3 h-3 animate-spin" />
                <span>{busyLabel}</span>
            </div>
        {/if}
        {#if $voiceActive}
            <div class="listening-pill" role="status" aria-live="polite">
                <span class="status-dot status-dot-listening" aria-hidden="true"></span>
                <Mic class="w-3 h-3" />
                <span>{$_('statusBar.listening')}</span>
            </div>
        {/if}
        {#if $pushToTalk.listening}
            <div class="listening-pill" role="status" aria-live="polite">
                <span class="status-dot status-dot-listening" aria-hidden="true"></span>
                <Mic class="w-3 h-3" />
                <span>{$_('statusBar.pushToTalkListening')}</span>
            </div>
        {/if}
        {#if $commandMode.active}
            <button
                type="button"
                class="command-pill"
                onclick={() => requestToggleCommandMode()}
                title={$_('statusBar.commandModeStop')}
            >
                <Radio class="w-3 h-3" />
                <span>{$_('statusBar.commandMode')}</span>
            </button>
        {/if}
    </div>

    <div class="flex items-center gap-3 text-muted">
        <span class="inline-flex items-center gap-1.5">
            <span
                class="status-dot"
                class:status-dot-active={searchReady}
                class:status-dot-idle={!searchReady}
                aria-hidden="true"
            ></span>
            {searchReady ? $_('statusBar.searchReady') : $_('statusBar.searchNotBuilt')}
        </span>

        <span
            class="inline-flex items-center gap-1.5"
            title={searchWatcherFoldersOnly ? searchWatcherMessage : undefined}
        >
            <span
                class="status-dot"
                class:status-dot-active={watcherWorking}
                class:status-dot-idle={!watcherWorking}
                aria-hidden="true"
            ></span>
            {searchWatcherLabel}
        </span>

        <span class="inline-flex items-center gap-1.5">
            <span
                class="status-dot"
                class:status-dot-working={searchIndexing}
                class:status-dot-idle={!searchIndexing}
                aria-hidden="true"
            ></span>
            {searchIndexing ? $_('statusBar.indexing') : $_('statusBar.idle')}
        </span>

<!--        <span>{$_('statusBar.files', { values: { count: searchIndexedFiles.toLocaleString() } })}</span>-->
<!--        {#if searchError}-->
<!--            <span class="text-error">{$_('statusBar.indexerIssue')}</span>-->
<!--        {/if}-->
    </div>
</footer>

<style>

    .trust-pill {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 2px 8px;
        border-radius: 999px;
        font-weight: 500;
        color: var(--color-accent);
        white-space: nowrap;
    }

    .status-dot {
        display: inline-block;
        width: 7px;
        height: 7px;
        border-radius: 50%;
        flex-shrink: 0;
        transition: background-color 220ms ease, opacity 220ms ease;
    }

    .status-dot-active {
        background: var(--color-success);
        box-shadow: 0 0 0 1px color-mix(in srgb, var(--color-success) 32%, transparent);
    }

    .status-dot-idle {
        background: var(--color-muted);
        opacity: 0.55;
    }

    .status-dot-working {
        background: var(--color-warning);
        box-shadow: 0 0 0 0 color-mix(in srgb, var(--color-warning) 80%, transparent);
        animation: status-flicker 900ms ease-in-out infinite;
    }

    .status-dot-listening {
        background: var(--color-error);
        box-shadow: 0 0 0 0 color-mix(in srgb, var(--color-error) 80%, transparent);
        animation: status-listening-flicker 700ms ease-in-out infinite;
    }

    @keyframes status-listening-flicker {
        0%,
        100% {
            opacity: 0.55;
            transform: scale(0.85);
            box-shadow: 0 0 0 0 color-mix(in srgb, var(--color-error) 18%, transparent);
        }
        50% {
            opacity: 1;
            transform: scale(1.45);
            box-shadow: 0 0 0 5px color-mix(in srgb, var(--color-error) 22%, transparent);
        }
    }

    .listening-pill {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 2px 8px;
        border-radius: 999px;
        font-size: 10.5px;
        font-weight: 600;
        color: var(--color-error);
        background: color-mix(in srgb, var(--color-error) 12%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-error) 36%, var(--color-border));
        white-space: nowrap;
    }

    .command-pill {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 2px 8px;
        border-radius: 999px;
        font-size: 10.5px;
        font-weight: 600;
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 12%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-accent) 34%, var(--color-border));
        white-space: nowrap;
        cursor: pointer;
        transition: background-color 160ms ease, border-color 160ms ease;
    }
    .command-pill:hover {
        background: color-mix(in srgb, var(--color-accent) 22%, transparent);
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
    }

    @keyframes status-flicker {
        0%,
        100% {
            opacity: 0.5;
            transform: scale(0.85);
            box-shadow: 0 0 0 0 color-mix(in srgb, var(--color-warning) 15%, transparent);
        }
        45% {
            opacity: 1;
            transform: scale(1.4);
            box-shadow: 0 0 0 5px color-mix(in srgb, var(--color-warning) 18%, transparent);
        }
        65% {
            opacity: 0.78;
            transform: scale(1.05);
            box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-warning) 30%, transparent);
        }
    }
</style>
