<script lang="ts">
    /*
      First-run modal introducing KeepItLocal's global hotkeys (search,
      clipboard, and — when enabled — voice commands).
      Modal pattern (not inline banner) so the user explicitly engages with
      the hint instead of skimming past it. Three dismissal paths:
        - Click "Got it" with "Don't show again" CHECKED   → persists, never shows again.
        - Click "Got it" with "Don't show again" UNCHECKED → hides this session only,
                                                            comes back on next launch.
        - Click backdrop / press Esc                       → same as the unchecked path
                                                            (user wasn't ready to commit).

      The "Don't show again" checkbox defaults to CHECKED because most users
      who see this once are done with it — but leaving the unchecked path
      open avoids feeling pushy.
    */
    import { onMount, onDestroy } from 'svelte';
    import { Search, Clipboard, Sparkles, X, Keyboard, Mic } from '@lucide/svelte';
    import { settings } from '$lib/stores/settings';
    import { _ } from 'svelte-i18n';

    /** Session-local dismissal — set when the user closes the modal without
     * checking "don't show again". Stops the modal from re-appearing within
     * the same app run; next launch surfaces it again unless the persistent
     * flag is also set. */
    let dismissedThisSession = $state(false);
    /** Persisted "permanently dismiss" preference, mirroring the
     * onboardingHotkeysShown setting. Defaults to checked because that's
     * what most users want after seeing the hint once. */
    let dontShowAgain = $state(true);

    /** Final visibility — open until the user has either persistently or
     * locally dismissed the modal. */
    let isOpen = $derived(!$settings.onboardingHotkeysShown && !dismissedThisSession);

    /** The "Got it" CTA — focused programmatically when the modal opens
     *  (replaces an `autofocus` attribute, which a11y tooling discourages). */
    let ctaEl = $state<HTMLButtonElement | null>(null);
    $effect(() => {
        if (isOpen) ctaEl?.focus();
    });

    function dismiss() {
        if (dontShowAgain) {
            // Persist via the settings store; cross-window broadcast handles
            // syncing the flag to any other open KeepItLocal window automatically.
            settings.update((s) => ({ ...s, onboardingHotkeysShown: true }));
        } else {
            dismissedThisSession = true;
        }
    }

    function onKeydown(event: KeyboardEvent) {
        if (!isOpen) return;
        if (event.key === 'Escape') {
            event.preventDefault();
            dismiss();
        }
        if (event.key === 'Enter') {
            // Enter confirms — same as clicking "Got it". Common convention
            // for one-button modals, and lets keyboard-only users dismiss
            // without reaching for the mouse.
            event.preventDefault();
            dismiss();
        }
    }

    onMount(() => {
        window.addEventListener('keydown', onKeydown);
    });

    onDestroy(() => {
        window.removeEventListener('keydown', onKeydown);
    });

    /** Render the user's actual configured shortcut, with a friendly fallback.
     * Strips the Tauri `CommandOrControl` token to plain `Ctrl` since this is
     * Windows-only for now and keys read cleaner that way. */
    function pretty(shortcut: string | undefined, fallback: string): string {
        const value = (shortcut ?? '').trim();
        const source = value.length > 0 ? value : fallback;
        return source.replace(/CommandOrControl/gi, 'Ctrl').replace(/\+/g, ' + ');
    }

    let searchShortcut = $derived(pretty($settings.commandOverlayShortcut, 'Ctrl+Alt+K'));
    let clipboardShortcut = $derived(
        pretty($settings.clipboardOverlayShortcut, 'Ctrl+Shift+V'),
    );
    let voiceShortcut = $derived(pretty($settings.voiceOverlayShortcut, 'Ctrl+Alt+V'));
    /** The voice overlay hotkey is optional (Settings → Voice can disable
     * it). Default is ON, so first-run users see all three cards — but
     * we still gate the card so a user who turned it off before the
     * onboarding modal first appeared doesn't see a hotkey that won't fire. */
    let showVoiceCard = $derived($settings.voiceOverlayEnabled !== false);
</script>

{#if isOpen}
    <!-- Backdrop catches clicks outside the modal panel for soft-dismiss
         (presentation role + a target check, so only true backdrop clicks
         dismiss). The panel itself carries the dialog semantics. We don't
         trap focus internally for v1 since the modal has only two
         interactive elements and Escape always works. -->
    <div
        class="ob-backdrop"
        role="presentation"
        onclick={(event) => { if (event.target === event.currentTarget) dismiss(); }}
    >
        <div
            class="ob-panel"
            role="dialog"
            aria-modal="true"
            aria-labelledby="ob-title"
            tabindex="-1"
        >
            <div class="ob-decor" aria-hidden="true"></div>

            <button class="ob-close" type="button" onclick={dismiss} aria-label={$_('tour.dismiss')}>
                <X class="ob-close-icon" />
            </button>

            <div class="ob-header">
                <div class="ob-header-icon" aria-hidden="true">
                    <Keyboard class="ob-header-keyboard" />
                </div>
                <div>
                    <div class="ob-eyebrow">
                        <Sparkles class="ob-eyebrow-icon" />
                        <span>{$_('tour.welcomeEyebrow')}</span>
                    </div>
                    <h2 id="ob-title" class="ob-title">{$_('tour.hotkeyTitle')}</h2>
                </div>
            </div>

            <p class="ob-intro">
                {$_('tour.hotkeyIntro')}
            </p>

            <div class="ob-hotkeys">
                <div class="ob-hotkey">
                    <div class="ob-icon">
                        <Search class="ob-hotkey-icon" />
                    </div>
                    <div class="ob-text">
                        <div class="ob-keys">
                            {#each searchShortcut.split(' + ') as part, i (i)}
                                {#if i > 0}<span class="ob-plus">+</span>{/if}
                                <kbd>{part}</kbd>
                            {/each}
                        </div>
                        <div class="ob-name">{$_('tour.quickSearch')}</div>
                        <div class="ob-label">
                            {$_('tour.quickSearchLabel')}
                        </div>
                    </div>
                </div>

                <div class="ob-hotkey">
                    <div class="ob-icon">
                        <Clipboard class="ob-hotkey-icon" />
                    </div>
                    <div class="ob-text">
                        <div class="ob-keys">
                            {#each clipboardShortcut.split(' + ') as part, i (i)}
                                {#if i > 0}<span class="ob-plus">+</span>{/if}
                                <kbd>{part}</kbd>
                            {/each}
                        </div>
                        <div class="ob-name">{$_('tour.clipboardHistory')}</div>
                        <div class="ob-label">
                            {$_('tour.clipboardHistoryLabel')}
                        </div>
                    </div>
                </div>

                {#if showVoiceCard}
                    <div class="ob-hotkey">
                        <div class="ob-icon">
                            <Mic class="ob-hotkey-icon" />
                        </div>
                        <div class="ob-text">
                            <div class="ob-keys">
                                {#each voiceShortcut.split(' + ') as part, i (i)}
                                    {#if i > 0}<span class="ob-plus">+</span>{/if}
                                    <kbd>{part}</kbd>
                                {/each}
                            </div>
                            <div class="ob-name">{$_('tour.voiceCommands')}</div>
                            <div class="ob-label">
                                {$_('tour.voiceCommandsLabel')}
                            </div>
                        </div>
                    </div>
                {/if}
            </div>

            <div class="ob-actions">
                <label class="ob-toggle">
                    <input type="checkbox" bind:checked={dontShowAgain} />
                    <span>{$_('tour.dontShowAgain')}</span>
                </label>

                <button class="ob-cta" type="button" onclick={dismiss} bind:this={ctaEl}>
                    {$_('tour.gotIt')}
                </button>
            </div>
        </div>
    </div>
{/if}

<style>
    .ob-backdrop {
        position: fixed;
        inset: 0;
        z-index: 999;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 24px;
        background: rgba(0, 0, 0, 0.55);
        backdrop-filter: blur(4px);
        animation: ob-backdrop-in 200ms cubic-bezier(0.2, 0.9, 0.3, 1);
    }

    @keyframes ob-backdrop-in {
        from {
            opacity: 0;
        }
        to {
            opacity: 1;
        }
    }

    .ob-panel {
        position: relative;
        width: 100%;
        max-width: 520px;
        padding: 24px 24px 20px;
        border-radius: 16px;
        border: 1px solid color-mix(in srgb, var(--color-accent, #10b981) 22%, var(--color-border, #262626));
        background: var(--color-panel, #141414);
        box-shadow:
            0 28px 64px rgba(0, 0, 0, 0.5),
            0 0 0 1px rgba(255, 255, 255, 0.03) inset;
        overflow: hidden;
        animation: ob-panel-in 280ms cubic-bezier(0.2, 0.9, 0.3, 1);
    }

    @keyframes ob-panel-in {
        from {
            opacity: 0;
            transform: translateY(12px) scale(0.96);
        }
        to {
            opacity: 1;
            transform: translateY(0) scale(1);
        }
    }

    /* Subtle teal glow in the top-left of the panel — gives the modal a
       touch of brand warmth without competing with the keyboard cards. */
    .ob-decor {
        pointer-events: none;
        position: absolute;
        inset: 0;
        background: radial-gradient(
            circle at 12% 0%,
            color-mix(in srgb, var(--color-accent, #10b981) 18%, transparent) 0%,
            transparent 45%
        );
    }

    .ob-close {
        position: absolute;
        top: 12px;
        right: 12px;
        width: 26px;
        height: 26px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 7px;
        border: 1px solid transparent;
        background: transparent;
        color: var(--color-muted, #737373);
        cursor: pointer;
        transition: background-color 120ms ease, color 120ms ease, border-color 120ms ease;
        z-index: 1;
    }

    .ob-close:hover {
        background: var(--color-panel-2, #1c1c1c);
        border-color: var(--color-border, #262626);
        color: var(--color-text, #e5e5e5);
    }

    :global(.ob-close-icon) {
        width: 14px;
        height: 14px;
    }

    .ob-header {
        position: relative;
        display: flex;
        align-items: flex-start;
        gap: 14px;
        margin-bottom: 14px;
    }

    .ob-header-icon {
        flex-shrink: 0;
        width: 44px;
        height: 44px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 11px;
        background: color-mix(in srgb, var(--color-accent, #10b981) 12%, var(--color-panel, #141414));
        border: 1px solid color-mix(in srgb, var(--color-accent, #10b981) 26%, var(--color-border, #262626));
        color: var(--color-accent, #10b981);
    }

    :global(.ob-header-keyboard) {
        width: 22px;
        height: 22px;
    }

    .ob-eyebrow {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        font-weight: 600;
        color: var(--color-accent, #10b981);
        margin-bottom: 4px;
    }

    :global(.ob-eyebrow-icon) {
        width: 12px;
        height: 12px;
    }

    .ob-title {
        position: relative;
        margin: 0;
        font-size: 18px;
        line-height: 1.3;
        font-weight: 600;
        letter-spacing: -0.01em;
        color: var(--color-text, #e5e5e5);
    }

    .ob-intro {
        position: relative;
        margin: 0 0 18px;
        font-size: 13px;
        line-height: 1.55;
        color: var(--color-text-secondary, #a3a3a3);
    }

    .ob-hotkeys {
        position: relative;
        display: flex;
        flex-direction: column;
        gap: 10px;
        margin-bottom: 18px;
    }

    .ob-hotkey {
        display: flex;
        align-items: flex-start;
        gap: 12px;
        padding: 12px;
        border-radius: 11px;
        border: 1px solid var(--color-border, #262626);
        background: var(--color-bg, #0a0a0a);
    }

    .ob-icon {
        flex-shrink: 0;
        width: 32px;
        height: 32px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 8px;
        background: var(--color-panel-2, #1c1c1c);
        color: var(--color-accent, #10b981);
    }

    :global(.ob-hotkey-icon) {
        width: 16px;
        height: 16px;
    }

    .ob-text {
        flex: 1;
        min-width: 0;
    }

    .ob-keys {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 4px;
        margin-bottom: 6px;
    }

    .ob-plus {
        color: var(--color-muted, #737373);
        font-size: 11px;
    }

    .ob-name {
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text, #e5e5e5);
        margin-bottom: 3px;
    }

    .ob-label {
        font-size: 12px;
        line-height: 1.5;
        color: var(--color-text-secondary, #a3a3a3);
    }

    .ob-actions {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding-top: 14px;
        border-top: 1px solid var(--color-border, #262626);
    }

    .ob-toggle {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        font-size: 12px;
        color: var(--color-text-secondary, #a3a3a3);
        cursor: pointer;
        user-select: none;
    }

    .ob-toggle input {
        margin: 0;
        width: 14px;
        height: 14px;
        accent-color: var(--color-accent, #10b981);
        cursor: pointer;
    }

    .ob-toggle:hover {
        color: var(--color-text, #e5e5e5);
    }

    .ob-cta {
        padding: 8px 18px;
        border-radius: 9px;
        border: 1px solid color-mix(in srgb, var(--color-accent, #10b981) 40%, var(--color-border, #262626));
        background: color-mix(in srgb, var(--color-accent, #10b981) 16%, var(--color-panel, #141414));
        color: var(--color-accent, #10b981);
        font-size: 13px;
        font-weight: 600;
        cursor: pointer;
        transition: background-color 140ms ease, border-color 140ms ease;
    }

    .ob-cta:hover {
        background: color-mix(in srgb, var(--color-accent, #10b981) 22%, var(--color-panel, #141414));
        border-color: color-mix(in srgb, var(--color-accent, #10b981) 65%, var(--color-border, #262626));
    }

    .ob-cta:focus-visible {
        outline: 2px solid color-mix(in srgb, var(--color-accent, #10b981) 70%, transparent);
        outline-offset: 2px;
    }

    kbd {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 26px;
        height: 24px;
        padding: 0 7px;
        border: 1px solid var(--color-border, #262626);
        border-radius: 6px;
        background: var(--color-panel-2, #1c1c1c);
        color: var(--color-text, #e5e5e5);
        font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
        font-size: 11.5px;
        font-weight: 500;
        line-height: 1;
        box-shadow: 0 1px 0 rgba(0, 0, 0, 0.45);
    }
</style>
