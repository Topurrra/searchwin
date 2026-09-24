<script lang="ts">
    import { invoke } from '@tauri-apps/api/core';
    import { onMount } from 'svelte';
    import {
        ArrowRight,
        BookOpen,
        Cpu,
        FileX,
        Library,
        Lock,
        Search,
        Shield,
        Sparkles,
        Clipboard,
        Keyboard,
        ShieldCheck,
    } from '@lucide/svelte';
    import { settings } from '$lib/stores/settings';
    import { _ } from 'svelte-i18n';
    import { ToolPage } from '$lib/ui';

    /** Pretty form of the user's actual hotkeys for the "Try these now"
     * cards — re-derives if they remap so the cards always show the
     * truth, not the defaults. */
    function pretty(shortcut: string | undefined, fallback: string): string {
        return (shortcut ?? fallback)
            .replace(/CommandOrControl/gi, 'Ctrl')
            .replace(/\+/g, ' + ');
    }
    let searchHotkey = $derived(pretty($settings.commandOverlayShortcut, 'CommandOrControl+Alt+K'));
    let clipboardHotkey = $derived(pretty($settings.clipboardOverlayShortcut, 'CommandOrControl+Shift+V'));

    let { selected = $bindable() }: { selected: string } = $props();

    const promises = [
        {
            icon: Lock,
            title: 'page.about.promises.local.title',
            text: 'page.about.promises.local.text',
        },
        {
            icon: Shield,
            title: 'page.about.promises.noAccount.title',
            text: 'page.about.promises.noAccount.text',
        },
        {
            icon: FileX,
            title: 'page.about.promises.privacy.title',
            text: 'page.about.promises.privacy.text',
        },
        {
            icon: Cpu,
            title: 'page.about.promises.backend.title',
            text: 'page.about.promises.backend.text',
        },
    ];

    const primaryActions = [
        {
            id: 'file-search',
            icon: Search,
            title: 'page.about.actions.fileSearch.title',
            text: 'page.about.actions.fileSearch.text',
        },
        {
            id: 'documentation',
            icon: BookOpen,
            title: 'page.about.actions.documentation.title',
            text: 'page.about.actions.documentation.text',
        },
        {
            id: 'privacy-guide',
            icon: Library,
            title: 'page.about.actions.privacyGuide.title',
            text: 'page.about.actions.privacyGuide.text',
        },
    ];

    const workflowSteps = [
        'page.about.workflowSteps.0',
        'page.about.workflowSteps.1',
        'page.about.workflowSteps.2',
    ];

    let halcyonText = $state('');
    let halcyonMark = $state('');
    let halcyonOpen = $state(false);
    let activePointerId: number | null = $state(null);
    let lastPointerX = $state(0);
    let lastPointerY = $state(0);
    let lastDirection: number | null = $state(null);
    let halcyonArmed = $state(false);
    let armTimeout: ReturnType<typeof setTimeout> | null = null;
    const armTimeoutMs = 20000;

    const minSwipeDistance = 12;
    const directionRight = 2;
    const directionDown = 1;
    const directionLeft = 3;
    const directionUp = 0;

    interface HalcyonResponse {
        unlocked: boolean;
        message: string | null;
        mark: string | null;
    }

    function halcyonClose() {
        halcyonOpen = false;
    }

    async function submitDirection(directionCode: number) {
        if (halcyonOpen) return;

        try {
            const result = await invoke<HalcyonResponse>('halcyon_probe', {
                directionCode,
            });

            if (result.unlocked && result.message) {
                halcyonArmed = false;
                halcyonText = result.message;
                halcyonMark = result.mark ?? '';
                halcyonOpen = true;
                clearArmTimeout();
            }
        } catch (error) {
            console.error(error);
        }
    }

    function clearArmTimeout() {
        if (armTimeout !== null) {
            clearTimeout(armTimeout);
            armTimeout = null;
        }
    }

    function resetArmTimeout() {
        clearArmTimeout();
        armTimeout = setTimeout(() => {
            halcyonArmed = false;
        }, armTimeoutMs);
    }

    async function halcyonArm() {
        try {
            await invoke('halcyon_reset');
        } catch (error) {
            console.error(error);
        }

        halcyonArmed = true;
        resetArmTimeout();
    }

    function halcyonStop() {
        halcyonArmed = false;
        clearArmTimeout();
    }

    function startGesture(event: PointerEvent) {
        if (!halcyonArmed) return;
        activePointerId = event.pointerId;
        lastPointerX = event.clientX;
        lastPointerY = event.clientY;
        lastDirection = null;
        resetArmTimeout();
    }

    function trackGesture(event: PointerEvent) {
        if (!halcyonArmed || activePointerId !== event.pointerId) return;

        const deltaX = event.clientX - lastPointerX;
        const deltaY = event.clientY - lastPointerY;

        if (Math.abs(deltaX) < minSwipeDistance && Math.abs(deltaY) < minSwipeDistance) return;

        const direction =
            Math.abs(deltaX) > Math.abs(deltaY)
                ? deltaX > 0 ? directionRight : directionLeft
                : deltaY > 0 ? directionDown : directionUp;

        if (direction === lastDirection) {
            lastPointerX = event.clientX;
            lastPointerY = event.clientY;
            return;
        }

        lastDirection = direction;
        submitDirection(direction);
        resetArmTimeout();

        lastPointerX = event.clientX;
        lastPointerY = event.clientY;
    }

    function endGesture() {
        activePointerId = null;
        lastDirection = null;
    }

    function halcyonKeydown(event: KeyboardEvent) {
        if (!halcyonArmed) return;

        const keyDirection =
            event.key === 'ArrowUp'
                ? 0
                : event.key === 'ArrowDown'
                    ? 1
                    : event.key === 'ArrowRight'
                        ? 2
                        : event.key === 'ArrowLeft'
                            ? 3
                            : null;

        if (keyDirection === null) return;

        event.preventDefault();
        event.stopPropagation();
        submitDirection(keyDirection);
        resetArmTimeout();
    }

    onMount(() => {
        window.addEventListener('keydown', halcyonKeydown);

        return () => {
            window.removeEventListener('keydown', halcyonKeydown);
            clearArmTimeout();
        };
    });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- Pointer handlers back an optional input affordance, not a UI control;
     all visible content on this page is fully operable without them. -->
<!-- The handlers + hidden trigger button live on this plain wrapper around
     ToolPage (position: relative) so migrating the header to the kit later
     doesn't disturb them. -->
<div
    class="about-root"
    style="position: relative;"
    onpointerdown={startGesture}
    onpointermove={trackGesture}
    onpointerup={endGesture}
    onpointercancel={endGesture}
>
    <button
            onclick={halcyonArm}
            onblur={halcyonStop}
            tabindex="-1"
            aria-label="about-corner"
            class="absolute left-1 top-1 w-3 h-3 opacity-0 cursor-default z-50"
    ></button>
    <ToolPage
        icon={Sparkles}
        iconTint="var(--color-accent)"
        title={$_('page.about.heroTitle')}
        description={$_('page.about.heroSubtitle')}
        width="wide"
        fill={false}
    >
        <!-- Jump-in cards (were the hero's right column). These cover the
             same destinations the old hero CTA buttons did, so the plain
             buttons were dropped to avoid duplicate navigation. -->
        <div class="grid gap-3 sm:grid-cols-3">
            {#each primaryActions as action}
                <button
                        onclick={() => (selected = action.id)}
                        class="group rounded-2xl border border-border bg-panel-2 p-4 text-left transition hover:border-accent/70 hover:bg-panel-3"
                >
                    <div class="flex items-center justify-between gap-3">
                        <div class="flex h-10 w-10 items-center justify-center rounded-xl border border-border bg-panel">
                            <action.icon class="h-5 w-5 text-accent" />
                        </div>
                        <ArrowRight class="h-4 w-4 text-muted transition group-hover:translate-x-0.5 group-hover:text-accent" />
                    </div>
                    <div class="mt-3 text-sm font-semibold">{$_(action.title)}</div>
                    <div class="mt-1 text-xs leading-5 text-muted">{$_(action.text)}</div>
                </button>
            {/each}
        </div>

    <!-- Three killer hotkeys — the headline workflows that turn KeepItLocal
         from "another utility app" into a daily-driver. Surfaces them
         right after the hero so first-time users know what's possible
         within 10 seconds of opening the app. -->
    <div class="rounded-2xl border border-border bg-panel p-4 md:p-5">
        <div class="mb-3 flex items-center justify-between">
            <div>
                <h2 class="text-base font-semibold">{$_('page.about.tryNow.title')}</h2>
                <p class="mt-1 text-xs text-muted">{$_('page.about.tryNow.subtitle')}</p>
            </div>
        </div>

        <div class="grid gap-3 lg:grid-cols-3">
            <!-- Quick Search overlay -->
            <button
                onclick={() => (selected = 'file-search')}
                class="group rounded-xl border border-border bg-panel-2 p-4 text-left hover:border-accent/60 transition-colors"
            >
                <div class="flex items-center gap-2">
                    <Search class="h-4 w-4 text-accent" />
                    <span class="text-sm font-semibold">{$_('page.about.tryNow.quickSearch.title')}</span>
                </div>
                <div class="mt-2 inline-flex items-center gap-1 rounded-lg border border-accent/25 bg-accent/10 px-2 py-1">
                    <Keyboard class="h-3 w-3 text-accent" />
                    <span class="text-[11px] font-mono text-accent">{searchHotkey}</span>
                </div>
                <p class="mt-2 text-xs leading-5 text-muted">
                    {$_('page.about.tryNow.quickSearch.text')}
                </p>
            </button>

            <!-- Clipboard overlay -->
            <button
                onclick={() => (selected = 'clipboard-history')}
                class="group rounded-xl border border-border bg-panel-2 p-4 text-left hover:border-accent/60 transition-colors"
            >
                <div class="flex items-center gap-2">
                    <Clipboard class="h-4 w-4 text-accent" />
                    <span class="text-sm font-semibold">{$_('page.about.tryNow.clipboard.title')}</span>
                </div>
                <div class="mt-2 inline-flex items-center gap-1 rounded-lg border border-accent/25 bg-accent/10 px-2 py-1">
                    <Keyboard class="h-3 w-3 text-accent" />
                    <span class="text-[11px] font-mono text-accent">{clipboardHotkey}</span>
                </div>
                <p class="mt-2 text-xs leading-5 text-muted">
                    {$_('page.about.tryNow.clipboard.text')}
                </p>
            </button>

            <!-- Snippets -->
            <button
                onclick={() => (selected = 'snippets')}
                class="group rounded-xl border border-border bg-panel-2 p-4 text-left hover:border-accent/60 transition-colors"
            >
                <div class="flex items-center gap-2">
                    <Sparkles class="h-4 w-4 text-accent" />
                    <span class="text-sm font-semibold">{$_('page.about.tryNow.snippets.title')}</span>
                </div>
                <div class="mt-2 inline-flex items-center gap-1 rounded-lg border border-accent/25 bg-accent/10 px-2 py-1">
                    <Keyboard class="h-3 w-3 text-accent" />
                    <span class="text-[11px] font-mono text-accent">{clipboardHotkey} → /trigger</span>
                </div>
                <p class="mt-2 text-xs leading-5 text-muted">
                    {@html $_('page.about.tryNow.snippets.text')}
                </p>
            </button>
        </div>
    </div>

    <!-- Trust strip — surfaces the local-first / encrypted / no-telemetry
         claims that the rest of the app makes implicitly. Reinforces the
         pitch with concrete language and a link to the Threat Model entry
         in the Privacy Guide. -->
    <div class="rounded-2xl border border-border bg-panel p-4 md:p-5">
        <div class="grid gap-3 sm:grid-cols-3">
            <div class="flex items-start gap-3">
                <div class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-accent/30 bg-accent/10 text-accent">
                    <ShieldCheck class="h-4 w-4" />
                </div>
                <div>
                    <div class="text-sm font-semibold">{$_('page.about.trust.encrypted.title')}</div>
                    <div class="mt-1 text-xs leading-5 text-muted">
                        {$_('page.about.trust.encrypted.text')}
                    </div>
                </div>
            </div>
            <div class="flex items-start gap-3">
                <div class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-accent/30 bg-accent/10 text-accent">
                    <Cpu class="h-4 w-4" />
                </div>
                <div>
                    <div class="text-sm font-semibold">{$_('page.about.trust.zeroNetwork.title')}</div>
                    <div class="mt-1 text-xs leading-5 text-muted">
                        {$_('page.about.trust.zeroNetwork.text')}
                    </div>
                </div>
            </div>
            <div class="flex items-start gap-3">
                <div class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-accent/30 bg-accent/10 text-accent">
                    <Lock class="h-4 w-4" />
                </div>
                <div>
                    <div class="text-sm font-semibold">{$_('page.about.trust.noAccount.title')}</div>
                    <div class="mt-1 text-xs leading-5 text-muted">
                        {$_('page.about.trust.noAccount.textBefore')}
                        <button
                            type="button"
                            onclick={() => (selected = 'privacy-guide')}
                            class="text-accent underline-offset-2 hover:underline"
                        >{$_('page.about.trust.noAccount.linkText')}</button>
                        {$_('page.about.trust.noAccount.textAfter')}
                    </div>
                </div>
            </div>
        </div>
    </div>

    <div class="grid gap-4 xl:grid-cols-[0.9fr_1.1fr]">
        <div class="rounded-2xl border border-border bg-panel p-4 md:p-5">
            <div class="flex items-start justify-between gap-3">
                <div>
                    <h2 class="text-base font-semibold">{$_('page.about.workflow.title')}</h2>
                    <p class="mt-1 text-xs text-muted">{$_('page.about.workflow.subtitle')}</p>
                </div>
                <div class="rounded-full border border-border bg-panel-2 px-2.5 py-1 text-xs text-muted">v1.0.0</div>
            </div>

            <div class="mt-4 space-y-2">
                {#each workflowSteps as step, index}
                    <div class="flex items-center gap-3 rounded-xl border border-border bg-panel-2 p-3">
                        <div class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-accent text-xs font-bold text-accent-contrast">
                            {index + 1}
                        </div>
                        <div class="text-sm">{$_(step)}</div>
                    </div>
                {/each}
            </div>
        </div>

        <div class="rounded-2xl border border-border bg-panel p-4 md:p-5">
            <div>
                <h2 class="text-base font-semibold">{$_('page.about.optimizes.title')}</h2>
                <p class="mt-1 text-xs text-muted">{$_('page.about.optimizes.subtitle')}</p>
            </div>

            <div class="mt-4 grid gap-3 sm:grid-cols-2">
                {#each promises as item}
                    <div class="rounded-xl border border-border bg-panel-2 p-3">
                        <item.icon class="mb-3 h-5 w-5 text-accent" />
                        <div class="text-sm font-medium">{$_(item.title)}</div>
                        <div class="mt-1 text-xs leading-5 text-muted">{$_(item.text)}</div>
                    </div>
                {/each}
            </div>
        </div>
    </div>

    <div class="rounded-2xl border border-border bg-panel p-4 md:p-5">
        <h2 class="text-base font-semibold">{$_('page.about.isNot.title')}</h2>
        <p class="mt-1 text-xs text-muted">{$_('page.about.isNot.subtitle')}</p>
        <div class="mt-4 grid gap-2 sm:grid-cols-2 xl:grid-cols-4 text-xs">
            <div class="rounded-xl border border-border bg-panel-2 p-3">
                <div class="font-medium mb-1">{$_('page.about.isNot.cloud.title')}</div>
                <div class="text-muted leading-relaxed">{$_('page.about.isNot.cloud.text')}</div>
            </div>
            <div class="rounded-xl border border-border bg-panel-2 p-3">
                <div class="font-medium mb-1">{$_('page.about.isNot.accountTrap.title')}</div>
                <div class="text-muted leading-relaxed">{$_('page.about.isNot.accountTrap.text')}</div>
            </div>
            <div class="rounded-xl border border-border bg-panel-2 p-3">
                <div class="font-medium mb-1">{$_('page.about.isNot.dataTrade.title')}</div>
                <div class="text-muted leading-relaxed">{$_('page.about.isNot.dataTrade.text')}</div>
            </div>
            <div class="rounded-xl border border-border bg-panel-2 p-3">
                <div class="font-medium mb-1">{$_('page.about.isNot.bloat.title')}</div>
                <div class="text-muted leading-relaxed">{$_('page.about.isNot.bloat.text')}</div>
            </div>
        </div>
    </div>

    <div class="flex flex-wrap items-center justify-between gap-2 text-xs text-muted">
        <div class="inline-flex items-center gap-1.5">
            {$_('page.about.footer.builtWith')}
            <img src="/rust.svg" alt="Rust" class="w-4 h-4" />
            <span>{$_('page.about.footer.tagline')}</span>
        </div>
    </div>
    </ToolPage>
</div>

{#if halcyonOpen}
    <div class="fixed inset-0 z-50 bg-black/50 backdrop-blur-sm flex items-center justify-center p-4">
        <div class="max-w-xl w-full bg-panel border border-border rounded-xl p-6 space-y-4 text-center">
            <div class="flex justify-center" aria-hidden="true">{@html halcyonMark}</div>
            <pre class="whitespace-pre-wrap text-sm leading-relaxed text-muted font-sans text-center">{halcyonText}</pre>
            <button
                    onclick={halcyonClose}
                    class="w-full rounded-lg border border-border bg-panel-2 hover:border-accent/70 transition-colors px-3 py-2 text-sm"
            >
                Close
            </button>
        </div>
    </div>
{/if}
