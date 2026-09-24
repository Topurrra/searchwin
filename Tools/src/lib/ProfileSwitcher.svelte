<script lang="ts">
    import { profiles, activeProfile, setActiveProfile } from '$lib/stores/profiles';
    import { ChevronDown, Check, Settings2, Layers } from '@lucide/svelte';
    import { _ } from 'svelte-i18n';

    let { selected = $bindable() }: { selected: string } = $props();

    let open = $state(false);
    let buttonEl: HTMLButtonElement | null = $state(null);

    function handleSelect(id: string) {
        setActiveProfile(id);
        open = false;
    }

    function openManager() {
        selected = 'profiles';
        open = false;
    }

    function handleClickOutside(e: MouseEvent) {
        const target = e.target as HTMLElement;
        if (open && !target.closest('[data-profile-switcher]')) {
            open = false;
        }
    }

    function handleEscape(e: KeyboardEvent) {
        if (e.key === 'Escape' && open) {
            open = false;
            buttonEl?.focus();
        }
    }
</script>

<svelte:window onclick={handleClickOutside} onkeydown={handleEscape} />

<div class="relative" data-profile-switcher>
    <button
            bind:this={buttonEl}
            onclick={() => (open = !open)}
            class="flex items-center gap-1.5 px-2 py-1 text-xs font-medium rounded
           text-text-secondary hover:text-text hover:bg-panel-2 transition-colors"
            aria-haspopup="menu"
            aria-expanded={open}
            title={$_('nav.switchProfile')}
    >
        <div class="text-sm font-semibold tracking-tight">{$_('nav.profile')}</div>
        <Layers class="w-3.5 h-3.5" />
        <span class="text-text">{$activeProfile?.name ?? $_('nav.allTools')}</span>
        <ChevronDown class="w-3 h-3 transition-transform {open ? 'rotate-180' : ''}" />
    </button>

    {#if open}
        <div
                class="fade-in absolute left-0 top-full mt-1.5 w-64 bg-panel border border-border rounded-lg shadow-xl overflow-hidden z-50"
                role="menu"
        >
            <div class="px-3 py-2 border-b border-border">
                <div class="text-xs font-semibold text-text-secondary uppercase tracking-wider">{$_('nav.profile')}</div>
            </div>

            <div class="max-h-80 overflow-y-auto py-1">
                {#each $profiles as p (p.id)}
                    <button
                            class="w-full text-left px-3 py-2 text-sm flex items-start gap-2 hover:bg-panel-2 transition-colors"
                            onclick={() => handleSelect(p.id)}
                            role="menuitem"
                    >
                        <div class="w-4 shrink-0 mt-0.5">
                            {#if p.id === $activeProfile?.id}
                                <Check class="w-4 h-4 text-accent" />
                            {/if}
                        </div>
                        <div class="flex-1 min-w-0">
                            <div class="flex items-center gap-1.5">
                                <span class="text-text {p.id === $activeProfile?.id ? 'font-medium' : ''}">{p.name}</span>
                                {#if !p.builtIn}
                                    <span class="text-[9px] text-muted uppercase">{$_('nav.customBadge')}</span>
                                {/if}
                            </div>
                            {#if p.description}
                                <div class="text-xs text-muted truncate">{p.description}</div>
                            {/if}
                        </div>
                    </button>
                {/each}
            </div>

            <button
                    class="w-full px-3 py-2 text-sm text-text-secondary hover:text-text hover:bg-panel-2 border-t border-border transition-colors flex items-center gap-2"
                    onclick={openManager}
                    role="menuitem"
            >
                <Settings2 class="w-3.5 h-3.5" />
                <span>{$_('nav.manageProfilesEllipsis')}</span>
            </button>
        </div>
    {/if}
</div>