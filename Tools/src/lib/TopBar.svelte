<script lang="ts">
    /*
      Top bar — the slim strip under the title bar. The 5 tool buttons
      (Search, Clipboard, Voice, File Search Index, Settings) and the
      KeepItLocal branding moved out per the v1 layout: branding lives
      in the TitleBar, tools live in the Sidebar / overlay / Settings.
      This bar now carries only the profile switcher (if optional packs
      are installed), the notifications bell, and the secondary info
      links (About / Docs / Privacy guide).
    */
    import NotificationsBell from './NotificationsBell.svelte';
    import { Info, Library, BookSearch, Settings as SettingsIcon } from '@lucide/svelte';
    import { _ } from 'svelte-i18n';

    let { selected = $bindable() }: { selected: string } = $props();

    // Profile switcher is ALWAYS visible — even with only the Core pack
    // installed, "All Tools" + any user-defined profiles are still valid
    // navigation. Gating it on `hasOptionalPacks` made the picker look
    // removed on fresh installs (user reported exactly this) and gave no
    // visual entry point to the Profiles concept. Loading it lazily keeps
    // the cold-paint cost off the critical path.
    let profileSwitcherComponent = $state<any>(null);

    $effect(() => {
        if (profileSwitcherComponent) return;
        void import('./ProfileSwitcher.svelte').then((module) => {
            profileSwitcherComponent = module.default;
        });
    });
</script>

<header class="h-10 border-b border-border bg-panel flex items-center justify-between gap-2 px-3 sm:px-4 shrink-0">
    <div class="flex min-w-0 items-center gap-3">
        {#if profileSwitcherComponent}
            {@const ProfileSwitcher = profileSwitcherComponent}
            <ProfileSwitcher bind:selected />
        {/if}
    </div>

    <div class="flex shrink-0 items-center gap-1">
        <NotificationsBell />
        <button
                onclick={() => (selected = 'settings')}
                class="p-2 rounded hover:bg-panel-2 transition-colors inline-flex {selected === 'settings' ? 'text-accent' : 'text-text-secondary'}"
                title={$_('nav.titleSettings')}
                aria-label={$_('nav.titleSettings')}
        >
            <SettingsIcon class="w-4 h-4" />
        </button>
        <button
            onclick={() => (selected = 'about')}
            class="hidden p-2 rounded hover:bg-panel-2 transition-colors sm:inline-flex {selected === 'about' ? 'text-accent' : 'text-text-secondary'}"
            title={$_('nav.titleAbout')}
            aria-label={$_('nav.titleAbout')}
        >
            <Info class="w-4 h-4" />
        </button>
        <button
            onclick={() => (selected = 'documentation')}
            class="hidden p-2 rounded hover:bg-panel-2 transition-colors md:inline-flex {selected === 'documentation' ? 'text-accent' : 'text-text-secondary'}"
            title={$_('nav.titleDocumentation')}
            aria-label={$_('nav.titleDocumentation')}
        >
            <BookSearch class="w-4 h-4" />
        </button>
        <button
            onclick={() => (selected = 'privacy-guide')}
            class="hidden p-2 rounded hover:bg-panel-2 transition-colors md:inline-flex {selected === 'privacy-guide' ? 'text-accent' : 'text-text-secondary'}"
            title={$_('nav.titlePrivacyGuide')}
            aria-label={$_('nav.titlePrivacyGuide')}
        >
            <Library class="w-4 h-4" />
        </button>
    </div>
</header>
