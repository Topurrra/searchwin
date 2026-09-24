<script lang="ts">
    /*
      First-run product tour. A 6-step modal that walks the user through
      KeepItLocal's headline features so they know what they bought into.

      Dismissal mechanics (same pattern as HotkeyOnboardingBanner):
        - Finish the last step                              → permanent dismiss (tourCompleted = true)
        - "Skip tour" with "Don't show again" CHECKED       → permanent dismiss
        - "Skip tour" with "Don't show again" UNCHECKED     → session-only dismiss
        - Esc / backdrop click                              → session-only dismiss
        - Settings → Onboarding & Tips → "Replay tour"      → flips tourCompleted back to false

      Each step is a static `{ icon, eyebrow, title, body, footnote? }` shape — no
      images, no animations beyond the panel pop. We rely on the user's actual
      configured shortcuts where relevant (steps 2 & 3) so what the tour says
      matches what they'll actually press.

      Redesign (kit): rebuilt on the component kit — kit <Button> for every
      control, kit <Kbd> for the shortcut caps, kit <Checkbox> for "don't show
      again". The panel is a plain overlay surface (var(--color-panel),
      --radius-overlay, --shadow-lg, hairline border): no decorative glow, the
      accent reserved for the eyebrow + primary action, per DESIGN.md.
    */
    import { onMount, onDestroy } from 'svelte';
    import {
        Search,
        Clipboard,
        Mic,
        Sparkles,
        ShieldCheck,
        Pin,
        Folder,
        X,
        ChevronLeft,
        ChevronRight,
        Check,
    } from '@lucide/svelte';
    import { settings } from '$lib/stores/settings';
    import { _ } from 'svelte-i18n';
    import { Button, Kbd, Checkbox } from '$lib/ui';

    /** Step index — 0..(steps.length-1). When the user clicks "Done" on the
     * last step we flip tourCompleted and the modal unmounts via $derived. */
    let stepIndex = $state(0);

    /** Session-only dismissal — Esc / backdrop / "Skip tour" without the
     * persistent checkbox. Re-shows on next launch unless the persistent
     * flag is also true. */
    let dismissedThisSession = $state(false);

    /** Persisted "permanently dismiss" preference toggled by the checkbox.
     * Defaults to true because most users finishing the tour are done with
     * it — but unchecking it lets curious users replay on next launch
     * without touching Settings. */
    let dontShowAgain = $state(true);

    /** Resolved visibility. Mirrors HotkeyOnboardingBanner — it's open until
     * the tour has been completed OR dismissed this session. */
    let isOpen = $derived(!$settings.tourCompleted && !dismissedThisSession);

    /** The primary CTA is focused when the tour opens. Because the CTA is now
     * the kit <Button> (whose `bind:this` would resolve to the component, not
     * the DOM node), we focus it by the forwarded `id` once the modal has
     * rendered — Svelte runs this effect after the DOM is patched. */
    $effect(() => {
        if (!isOpen) return;
        document.getElementById('tour-primary-cta')?.focus();
    });

    /** Pretty the user's actual configured shortcuts for steps 2/3. Strips
     * `CommandOrControl` (Tauri's cross-OS token) to plain `Ctrl` since
     * KeepItLocal is Windows-only for now. */
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

    /** Step content. Kept as a $derived so the shortcut strings AND the
     * translated copy re-evaluate if the user changes their hotkeys or
     * switches locale mid-tour. */
    let steps = $derived([
        {
            icon: Sparkles,
            eyebrow: $_('tour.step1Eyebrow'),
            title: $_('tour.step1Title'),
            body: $_('tour.step1Body'),
            footnote: $_('tour.step1Footnote'),
        },
        {
            icon: Search,
            eyebrow: $_('tour.step2Eyebrow'),
            title: $_('tour.step2Title'),
            body: $_('tour.step2Body'),
            shortcut: searchShortcut,
            shortcutLabel: $_('tour.tryItNow'),
        },
        {
            icon: Clipboard,
            eyebrow: $_('tour.step3Eyebrow'),
            title: $_('tour.step3Title'),
            body: $_('tour.step3Body'),
            shortcut: clipboardShortcut,
            shortcutLabel: $_('tour.tryItNow'),
        },
        {
            icon: Mic,
            eyebrow: $_('tour.step4Eyebrow'),
            title: $_('tour.step4Title'),
            body: $_('tour.step4Body'),
            shortcut: voiceShortcut,
            shortcutLabel: $_('tour.tryItNow'),
        },
        {
            icon: Pin,
            eyebrow: $_('tour.step5Eyebrow'),
            title: $_('tour.step5Title'),
            body: $_('tour.step5Body'),
            footnote: $_('tour.step5Footnote'),
            featureChips: [
                { icon: Pin, label: $_('tour.step5ChipPinned') },
                { icon: ShieldCheck, label: $_('tour.step5ChipSensitive') },
            ],
        },
        {
            icon: Folder,
            eyebrow: $_('tour.step6Eyebrow'),
            title: $_('tour.step6Title'),
            body: $_('tour.step6Body'),
            footnote: $_('tour.step6Footnote'),
        },
    ]);

    let isLast = $derived(stepIndex === steps.length - 1);
    let isFirst = $derived(stepIndex === 0);

    function next() {
        if (isLast) {
            finish();
            return;
        }
        stepIndex = Math.min(stepIndex + 1, steps.length - 1);
    }

    function back() {
        stepIndex = Math.max(stepIndex - 1, 0);
    }

    function jumpTo(target: number) {
        if (target < 0 || target >= steps.length) return;
        stepIndex = target;
    }

    /** Finish path — clicked "Done" on last step. Always permanently
     * dismisses, regardless of the checkbox (the checkbox only affects
     * the Skip path; a user who walked through every step has clearly
     * seen the tour).
     *
     * We also suppress the standalone HotkeyOnboardingBanner because steps
     * 2 + 3 already cover that exact content — without this, the user gets
     * a second modal stacked behind the tour on first install. Users can
     * still re-trigger either popup independently from Settings → Onboarding. */
    function finish() {
        settings.update((s) => ({
            ...s,
            tourCompleted: true,
            onboardingHotkeysShown: true,
        }));
    }

    /** Skip path — user clicked "Skip tour", Esc, or backdrop. Honors the
     * "Don't show again" checkbox so they can dismiss without losing
     * the chance to see it again on next launch. When persistently
     * dismissing, also suppress the hotkey banner for the same reason as
     * `finish()` above. */
    function skip() {
        if (dontShowAgain) {
            settings.update((s) => ({
                ...s,
                tourCompleted: true,
                onboardingHotkeysShown: true,
            }));
        } else {
            dismissedThisSession = true;
        }
    }

    function onKeydown(event: KeyboardEvent) {
        if (!isOpen) return;
        if (event.key === 'Escape') {
            event.preventDefault();
            skip();
        }
        if (event.key === 'ArrowRight') {
            event.preventDefault();
            next();
        }
        if (event.key === 'ArrowLeft') {
            event.preventDefault();
            back();
        }
        if (event.key === 'Enter') {
            event.preventDefault();
            next();
        }
    }

    onMount(() => {
        window.addEventListener('keydown', onKeydown);
    });

    onDestroy(() => {
        window.removeEventListener('keydown', onKeydown);
    });
</script>

{#if isOpen}
    {@const step = steps[stepIndex]}
    {@const StepIcon = step.icon}
    <div
        class="tour-backdrop"
        role="presentation"
        onclick={(event) => { if (event.target === event.currentTarget) skip(); }}
    >
        <div
            class="tour-panel"
            role="dialog"
            aria-modal="true"
            aria-labelledby="tour-title"
            tabindex="-1"
        >
            <!-- Close (top-right) — kit ghost icon button, in a positioned
                 slot so the modal's own scoped style can place it. -->
            <div class="tour-close-slot">
                <Button
                    variant="ghost"
                    size="sm"
                    iconOnly
                    icon={X}
                    onclick={skip}
                    aria-label={$_('tour.skipTour')}
                    title={$_('tour.skipTourTooltip')}
                />
            </div>

            <!-- Header — icon tile + eyebrow + title. -->
            <div class="tour-header">
                <div class="tour-icon-tile" aria-hidden="true">
                    <StepIcon class="tour-step-icon" />
                </div>
                <div class="tour-header-text">
                    <div class="tour-eyebrow">{step.eyebrow}</div>
                    <h2 id="tour-title" class="tour-title">{step.title}</h2>
                </div>
            </div>

            <p class="tour-body">{step.body}</p>

            <!-- Shortcut callout for steps 2-4. Renders the user's actual
                 configured shortcut via the kit <Kbd>, so what they see
                 matches what they'll press. -->
            {#if step.shortcut}
                <div class="tour-shortcut">
                    <span class="tour-shortcut-label">{step.shortcutLabel ?? $_('tour.shortcut')}</span>
                    <Kbd keys={step.shortcut} />
                </div>
            {/if}

            <!-- Optional feature-chip row (used by step 5). -->
            {#if step.featureChips}
                <div class="tour-chips">
                    {#each step.featureChips as chip}
                        {@const ChipIcon = chip.icon}
                        <span class="tour-chip">
                            <ChipIcon class="tour-chip-icon" />
                            {chip.label}
                        </span>
                    {/each}
                </div>
            {/if}

            {#if step.footnote}
                <p class="tour-footnote">{step.footnote}</p>
            {/if}

            <!-- Footer — progress dots on the left, controls on the right.
                 Dots double as a navigator (click to jump). -->
            <div class="tour-footer">
                <div class="tour-progress" role="tablist" aria-label={$_('tour.tourProgress')}>
                    {#each steps as _step, i (i)}
                        <button
                            type="button"
                            class="tour-dot"
                            class:active={i === stepIndex}
                            class:past={i < stepIndex}
                            role="tab"
                            aria-selected={i === stepIndex}
                            aria-label={$_('tour.goToStep', { values: { step: i + 1 } })}
                            onclick={() => jumpTo(i)}
                        ></button>
                    {/each}
                </div>

                <div class="tour-actions">
                    {#if isFirst}
                        <span class="tour-dont-show" title={$_('tour.dontShowAgainTooltip')}>
                            <Checkbox
                                bind:checked={dontShowAgain}
                                label={$_('tour.dontShowAgainShort')}
                            />
                        </span>
                    {/if}

                    <Button variant="ghost" onclick={skip}>
                        {isLast ? $_('nav.close') : $_('tour.skipTour')}
                    </Button>

                    {#if !isFirst}
                        <Button
                            variant="secondary"
                            icon={ChevronLeft}
                            onclick={back}
                            aria-label={$_('tour.previousStep')}
                            title={$_('tour.previousTooltip')}
                        >
                            {$_('tour.back')}
                        </Button>
                    {/if}

                    {#if isLast}
                        <Button
                            variant="primary"
                            id="tour-primary-cta"
                            icon={Check}
                            onclick={next}
                            title={$_('tour.finishTooltip')}
                        >
                            {$_('tour.done')}
                        </Button>
                    {:else}
                        <Button
                            variant="primary"
                            id="tour-primary-cta"
                            iconTrailing={ChevronRight}
                            onclick={next}
                            title={$_('tour.nextTooltip')}
                        >
                            {$_('tour.next')}
                        </Button>
                    {/if}
                </div>
            </div>
        </div>
    </div>
{/if}

<style>
    /* The tour shows before the hotkey banner, so in practice they're never
       on screen together, but staying above 900 keeps either out of reach
       of a stray dropdown. */
    .tour-backdrop {
        position: fixed;
        inset: 0;
        z-index: 999;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 24px;
        background: color-mix(in srgb, var(--color-bg) 55%, transparent);
        backdrop-filter: blur(6px);
        animation: tour-backdrop-in 200ms var(--ease-out, ease);
    }

    @keyframes tour-backdrop-in {
        from {
            opacity: 0;
        }
        to {
            opacity: 1;
        }
    }

    .tour-panel {
        position: relative;
        width: 100%;
        max-width: 540px;
        padding: 24px 24px 18px;
        border-radius: var(--radius-overlay, 16px);
        border: 1px solid var(--color-border);
        background: var(--color-panel);
        box-shadow: var(--shadow-lg);
        animation: tour-panel-in 280ms var(--ease-out, ease);
    }

    @keyframes tour-panel-in {
        from {
            opacity: 0;
            transform: translateY(12px) scale(0.97);
        }
        to {
            opacity: 1;
            transform: translateY(0) scale(1);
        }
    }

    .tour-close-slot {
        position: absolute;
        top: 12px;
        right: 12px;
        z-index: 1;
    }

    /* ─── Header ────────────────────────────────────────────────── */
    .tour-header {
        display: flex;
        align-items: flex-start;
        gap: 14px;
        margin-bottom: 14px;
        /* keep the title clear of the close button */
        padding-right: 36px;
    }

    .tour-icon-tile {
        flex-shrink: 0;
        width: 48px;
        height: 48px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: var(--radius-card, 12px);
        background: color-mix(in srgb, var(--color-accent) 14%, var(--color-panel-2));
        border: 1px solid color-mix(in srgb, var(--color-accent) 28%, var(--color-border));
        color: var(--color-accent);
    }

    :global(.tour-step-icon) {
        width: 24px;
        height: 24px;
    }

    .tour-header-text {
        flex: 1;
        min-width: 0;
    }

    .tour-eyebrow {
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.07em;
        font-weight: 600;
        color: var(--color-accent);
        margin-bottom: 4px;
    }

    .tour-title {
        margin: 0;
        font-size: 19px;
        line-height: 1.3;
        font-weight: 600;
        letter-spacing: -0.01em;
        color: var(--color-text);
    }

    .tour-body {
        margin: 0 0 14px;
        font-size: 13.5px;
        line-height: 1.6;
        color: var(--color-text-secondary);
    }

    /* ─── Shortcut callout ──────────────────────────────────────── */
    /* Neutral panel-2 surface, hairline border, accent label. The keys
       are the kit <Kbd>. */
    .tour-shortcut {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 11px 14px;
        margin-bottom: 14px;
        border-radius: var(--radius-card, 12px);
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
    }

    .tour-shortcut-label {
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        font-weight: 600;
        color: var(--color-accent);
    }

    /* ─── Feature chips ─────────────────────────────────────────── */
    .tour-chips {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
        margin-bottom: 14px;
    }

    .tour-chip {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 5px 10px;
        font-size: 12px;
        border-radius: 999px;
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text);
    }

    :global(.tour-chip-icon) {
        width: 12px;
        height: 12px;
        color: var(--color-accent);
    }

    .tour-footnote {
        margin: 0 0 16px;
        font-size: 12px;
        line-height: 1.5;
        color: var(--color-muted);
        font-style: italic;
    }

    /* ─── Footer ────────────────────────────────────────────────── */
    .tour-footer {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding-top: 16px;
        border-top: 1px solid var(--color-border);
    }

    /* Progress dots. Same language as the welcome wizard: neutral base,
       accent fill for past/active, active widens to a pill. Buttons so
       users can jump back/forward via click. */
    .tour-progress {
        display: inline-flex;
        align-items: center;
        gap: 6px;
    }

    .tour-dot {
        width: 7px;
        height: 7px;
        border-radius: 999px;
        border: none;
        padding: 0;
        background: var(--color-panel-3);
        cursor: pointer;
        transition:
            width var(--dur-micro, 130ms) var(--ease-out, ease),
            background-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }

    .tour-dot.past {
        background: color-mix(in srgb, var(--color-accent) 50%, var(--color-panel-3));
    }

    .tour-dot.active {
        width: 20px;
        border-radius: 4px;
        background: var(--color-accent);
    }

    .tour-dot:hover:not(.active) {
        background: color-mix(in srgb, var(--color-accent) 35%, var(--color-panel-3));
    }

    .tour-actions {
        display: inline-flex;
        align-items: center;
        gap: 8px;
    }

    .tour-dont-show {
        display: inline-flex;
        align-items: center;
        margin-right: 4px;
        font-size: 12px;
    }

    @media (prefers-reduced-motion: reduce) {
        .tour-backdrop,
        .tour-panel {
            animation: none;
        }
        .tour-dot {
            transition: none;
        }
    }
</style>
