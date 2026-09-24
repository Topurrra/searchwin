<script lang="ts">

    import { invoke } from '@tauri-apps/api/core';
    import { get } from 'svelte/store';
    import { Search, Clipboard as ClipboardIcon, Mic, History, Code2 } from '@lucide/svelte';
    import {
        commandActiveTab,
        commandWorkspaceTab,
        commandWorkspaceClipboardView,
    } from '$lib/stores/categoryNav';
    import FileSearch from '$lib/tools/Search/FileSearch.svelte';
    import ClipboardHistory from '$lib/tools/Utils/ClipboardHistory.svelte';
    import VoiceToText from '$lib/tools/Utils/VoiceToText.svelte';
    import Snippets from '$lib/tools/Utils/Snippets.svelte';


    let { selected = $bindable('command') }: { selected: string } = $props();

    type Tab = 'search' | 'clipboard' | 'voice';
    const TABS: { id: Tab; label: string; icon: typeof Search; toolId: string }[] = [
        { id: 'search', label: 'Search', icon: Search, toolId: 'file-search' },
        { id: 'clipboard', label: 'Clipboard', icon: ClipboardIcon, toolId: 'clipboard-history' },
        { id: 'voice', label: 'Voice', icon: Mic, toolId: 'voice-to-text' },
    ];

    let activeTab = $state<Tab>(get(commandWorkspaceTab));
    let clipboardView = $state<'history' | 'snippets'>(get(commandWorkspaceClipboardView));

    // Honor a router hand-off (which tab / sub-view to open), then clear it.
    $effect(() => {
        const pending = $commandActiveTab;
        if (!pending) return;
        if (pending === 'snippets') {
            activeTab = 'clipboard';
            commandWorkspaceTab.set(activeTab);
            clipboardView = 'snippets';
            commandWorkspaceClipboardView.set(clipboardView);
        } else {
            activeTab = pending;
            commandWorkspaceTab.set(activeTab);
        }
        commandActiveTab.set(null);
    });

    function pickTab(tab: Tab) {
        activeTab = tab;
        commandWorkspaceTab.set(tab);
        const toolId = TABS.find((t) => t.id === tab)?.toolId;
        // Frecency so the command palette's Suggested Tools still learns the
        // workflow even though selection happens inside this page.
        if (toolId) {
            void invoke('record_frecency_launch', { kind: 'tool', path: toolId }).catch(() => {});
        }
    }

    function pickClipboardView(view: 'history' | 'snippets') {
        clipboardView = view;
        commandWorkspaceClipboardView.set(view);
        void invoke('record_frecency_launch', {
            kind: 'tool',
            path: view === 'snippets' ? 'snippets' : 'clipboard-history',
        }).catch(() => {});
    }
</script>

<section class="cmdw">
    <div class="cmdw-tabs" role="tablist" aria-label="Command workflows">
        {#each TABS as tab (tab.id)}
            {@const Icon = tab.icon}
            <button
                type="button"
                role="tab"
                class="cmdw-tab"
                class:is-active={activeTab === tab.id}
                aria-selected={activeTab === tab.id}
                onclick={() => pickTab(tab.id)}
            >
                <Icon class="cmdw-tab-ico" />
                <span>{tab.label}</span>
            </button>
        {/each}

        {#if activeTab === 'clipboard'}
            <div class="cmdw-sub" role="group" aria-label="Clipboard view">
                <button
                    type="button"
                    class="cmdw-subtab"
                    class:is-active={clipboardView === 'history'}
                    aria-pressed={clipboardView === 'history'}
                    onclick={() => pickClipboardView('history')}
                >
                    <History class="cmdw-subtab-ico" />
                    History
                </button>
                <button
                    type="button"
                    class="cmdw-subtab"
                    class:is-active={clipboardView === 'snippets'}
                    aria-pressed={clipboardView === 'snippets'}
                    onclick={() => pickClipboardView('snippets')}
                >
                    <Code2 class="cmdw-subtab-ico" />
                    Snippets
                </button>
            </div>
        {/if}
    </div>

    <div class="cmdw-body">
        {#if activeTab === 'search'}
            <FileSearch bind:selected />
        {:else if activeTab === 'clipboard'}
            {#if clipboardView === 'snippets'}
                <Snippets />
            {:else}
                <ClipboardHistory />
            {/if}
        {:else if activeTab === 'voice'}
            <VoiceToText bind:selected />
        {/if}
    </div>
</section>

<style>
    .cmdw {
        display: flex;
        flex-direction: column;
        min-height: 100%;
    }
    /* Sticky tab strip — stays put while the active tool's content scrolls in
       the page's <main>. Opaque bg so scrolled content hides behind it. */
    .cmdw-tabs {
        position: sticky;
        top: 0;
        z-index: 5;
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 10px 16px 0;
        background: var(--color-bg);
        border-bottom: 1px solid var(--color-border);
    }
    .cmdw-tab {
        display: inline-flex;
        align-items: center;
        gap: 7px;
        padding: 9px 14px;
        margin-bottom: -1px; /* overlap the strip border for an underline tab */
        background: transparent;
        border: none;
        border-bottom: 2px solid transparent;
        color: var(--color-text-secondary);
        font-size: 13.5px;
        font-weight: 500;
        cursor: pointer;
        transition:
            color var(--dur-micro, 130ms) var(--ease-out, ease),
            border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .cmdw-tab:hover:not(.is-active) {
        color: var(--color-text);
    }
    .cmdw-tab.is-active {
        color: var(--color-accent);
        border-bottom-color: var(--color-accent);
    }
    .cmdw :global(.cmdw-tab-ico) {
        width: 16px;
        height: 16px;
    }
    /* Clipboard History / Snippets sub-toggle — sits at the right of the tab
       strip as a compact segmented control. */
    .cmdw-sub {
        margin-left: auto;
        display: inline-flex;
        align-items: center;
        gap: 2px;
        margin-bottom: 7px;
        padding: 2px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 999px;
    }
    .cmdw-subtab {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        padding: 4px 11px;
        border: none;
        border-radius: 999px;
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;
    }
    .cmdw-subtab.is-active {
        background: var(--color-panel);
        color: var(--color-text);
        box-shadow: 0 1px 2px rgba(0, 0, 0, 0.18);
    }
    .cmdw :global(.cmdw-subtab-ico) {
        width: 13px;
        height: 13px;
    }
    .cmdw-body {
        flex: 1;
        min-width: 0;
    }
</style>
