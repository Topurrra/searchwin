<script lang="ts">
    /*
      WelcomeSetup — multi-step first-run wizard (Welcome 2.0, Wave 7 2026-05-28).

      Step flow (the user actually sees):
        1. Intro             — what KeepItLocal is + ideology
        2. Pillar: Search    — local file search marketing
        3. Pillar: Clipboard — clipboard-with-memory marketing
        4. Pillar: Voice     — offline dictation marketing
        5. Catalog tour      — every tool category on one screen (NEW)
        6. Privacy promise   — explicit local-first guarantee (NEW)
        7. Pack picker       — toggle the optional packs you want now
        8. Magic moment      — "press Ctrl+Alt+K right now" (NEW, fires confetti)

      Wave 7 also maximizes the window on mount so the welcome reads as a
      "real" experience, not a modal in a small window — mirrors how
      Raycast / Linear / Cron handle first-run onboarding.
    */
    import { onDestroy } from 'svelte';
    import { quintOut } from 'svelte/easing';
    import { get } from 'svelte/store';
    import type { TransitionConfig } from 'svelte/transition';
    import confetti from 'canvas-confetti';
    import { invoke } from '@tauri-apps/api/core';
    import { completeWelcome } from '$lib/stores/onboarding';
    import { settings } from '$lib/stores/settings';
    import {
        enabledPackIds,
        saveInitialToolPackSelection,
    } from '$lib/stores/toolPacks';
    import { toast } from '$lib/stores/toasts';
    import { toolScreens, toolIdsByCategory } from '$lib/appScreens';
    import type { ToolPackId } from '$lib/appScreens';
    import {
        Shield,
        ShieldCheck,
        Search,
        Clipboard as ClipboardIcon,
        Mic,
        Wrench,
        Code,
        Lock,
        FileText,
        ImageIcon,
        Video,
        Files,
        Timer,
        ArrowRight,
        ArrowLeft,
        Check,
        Sparkles,
        EyeOff,
        WifiOff,
        UserX,
        type Icon as LucideIcon,
    } from '@lucide/svelte';

    // ─── Step model ───────────────────────────────────────────────
    // Wave 7 (2026-05-28): added 'catalog', 'privacy', and 'magic'
    // step kinds to expand the wizard into a Raycast-grade onboarding
    // experience. 'packs' moved earlier in the flow so the user picks
    // their toolset BEFORE seeing the magic-moment Ctrl+Alt+K demo —
    // that way the magic moment uses their own selected tools when
    // they later press the hotkey for real.
    type StepKind = 'intro' | 'pillar' | 'catalog' | 'privacy' | 'packs' | 'magic';
    interface StepDef {
        id: string;
        kind: StepKind;
        title: string;
        subtitle: string;
        icon: typeof LucideIcon;
        color: string;
        ctaLabel?: string; // overrides the default "Continue"
    }

    const STEPS: StepDef[] = [
        {
            id: 'intro',
            kind: 'intro',
            title: 'Private work should stay private.',
            subtitle:
                'KeepItLocal brings essential productivity tools directly to your desktop. No cloud uploads. No telemetry. Just fast, local-first power.',
            icon: Shield,
            color: '#10b981',
        },
        {
            id: 'pillar-search',
            kind: 'pillar',
            title: 'Search anything. Stays on your machine.',
            subtitle:
                'A local index makes tens of thousands of files searchable in milliseconds — under your control, never uploaded. Global hotkey, instant results, zero network calls.',
            icon: Search,
            color: '#10b981',
        },
        {
            id: 'pillar-clipboard',
            kind: 'pillar',
            title: 'Your clipboard, with memory.',
            subtitle:
                'Every copy lives in encrypted local history. Pin the essentials, expand snippets with triggers, paste back into the app you came from.',
            icon: ClipboardIcon,
            color: '#a855f7',
        },
        {
            id: 'pillar-voice',
            kind: 'pillar',
            title: 'Dictate without the cloud.',
            subtitle:
                'Offline speech-to-text. Hit a hotkey, speak, the transcript lands wherever your cursor is. Your voice never leaves the device.',
            icon: Mic,
            color: '#06b6d4',
        },
        // Wave 7.5 (2026-05-28): the standalone catalog step was
        // merged into the pack picker — they were visually showing
        // the same 10 categories twice. The pack picker now carries
        // both the "here's everything you get" message AND the
        // selection toggles, with taglines + tool counts inline.
        {
            id: 'privacy',
            kind: 'privacy',
            title: 'Your promise from us, in writing.',
            subtitle:
                'Three rules KeepItLocal will never break. Everything on this page is enforced by how the app is built, not just a privacy policy.',
            icon: ShieldCheck,
            color: '#10b981',
        },
        {
            id: 'packs',
            kind: 'packs',
            title: 'Pick what you want now.',
            // Wave 7.8.1 (2026-05-28): subtitle shortened from a
            // 180-char paragraph to a one-liner so the hero stays
            // compact and the actions row never gets pushed off-screen
            // by hero wrapping on narrower windows.
            subtitle: 'Core is always on. Toggle any pack — change anytime in Settings.',
            icon: Sparkles,
            color: '#f59e0b',
            ctaLabel: 'Almost done',
        },
        {
            id: 'magic',
            kind: 'magic',
            title: 'You\'re all set!',
            subtitle:
                'Press Ctrl + Alt + K from anywhere in Windows to summon KeepItLocal. Type to search, hit Enter to act, Esc to dismiss. That\'s it.',
            icon: Sparkles,
            color: '#10b981',
            ctaLabel: 'Enter KeepItLocal',
        },
    ];

    // Wave 7.5 (2026-05-28): the standalone CATALOG constant +
    // countToolsForCategory helper were removed when the catalog tour
    // step was merged into the pack picker. The merged step uses
    // WELCOME_PACKS' own `catalogCategories` field + `countToolsForPack`
    // (defined below) so a pack and its tool count live together —
    // no risk of the two lists drifting.

    // ─── Wizard state ─────────────────────────────────────────────
    let stepIndex = $state(0);
    let direction = $state<1 | -1>(1);
    let entering = $state(false);
    let prefersReducedMotion = $state(false);
    /** Wave 7 (2026-05-28): canvas-confetti needs a real <canvas> in the
     *  DOM. We use the default global confetti() which mounts its own
     *  canvas — easier than wiring up a bound element. The store flag is
     *  just to avoid double-firing on hot-reload or re-entry. */
    let confettiFired = $state(false);

    /** Honor user's OS-level motion preference. Power users + a11y
     *  benefit from instant swaps. */
    $effect(() => {
        if (typeof window === 'undefined') return;
        const mq = window.matchMedia('(prefers-reduced-motion: reduce)');
        const apply = () => {
            prefersReducedMotion = mq.matches;
        };
        apply();
        mq.addEventListener('change', apply);
        return () => mq.removeEventListener('change', apply);
    });

    /** Wave 7.9 (2026-05-28): welcome now runs in its own dedicated
     *  Tauri window (label "welcome"), born `maximized: true` at OS
     *  creation time. No JS-side maximize() needed — the window opens
     *  at the right size on the very first frame, which is what
     *  eliminates the "small dark frame → maximize animation → welcome
     *  paints" flash that the in-main implementation (pre-7.9)
     *  produced. The maximize() call that used to live here (and its
     *  getCurrentWebviewWindow import) have been removed — Rust owns
     *  the welcome window's geometry now, not JS. */

    /** Wave 7 (2026-05-28): fire the confetti burst when the user
     *  reaches the magic-moment / finale step. Uses canvas-confetti's
     *  default global mode (it appends its own <canvas> to body and
     *  cleans up automatically). Two staggered bursts from the bottom
     *  corners give the "stadium" effect Raycast uses — feels
     *  celebratory without being chaotic. Respects prefers-reduced-
     *  motion: no confetti, no flash. */
    function fireConfetti() {
        if (confettiFired) return;
        if (prefersReducedMotion) return;
        confettiFired = true;
        const defaults = {
            startVelocity: 35,
            spread: 70,
            ticks: 220,
            zIndex: 9999,
            disableForReducedMotion: true,
        };
        // Left burst.
        void confetti({
            ...defaults,
            particleCount: 80,
            origin: { x: 0.15, y: 0.85 },
            colors: ['#10b981', '#a855f7', '#06b6d4', '#f59e0b'],
        });
        // Right burst, 150 ms later — gives the "two cannons" feel
        // instead of one symmetrical poof.
        setTimeout(() => {
            void confetti({
                ...defaults,
                particleCount: 80,
                origin: { x: 0.85, y: 0.85 },
                colors: ['#10b981', '#a855f7', '#06b6d4', '#f59e0b'],
            });
        }, 150);
    }

    /** Reactive: as soon as the wizard advances to the magic step,
     *  fire the confetti. Tied to step index so a Back-then-Forward
     *  doesn't fire twice (the latch on `confettiFired` handles it). */
    $effect(() => {
        if (STEPS[stepIndex]?.kind === 'magic') {
            // Slight delay so the slide-in finishes first — confetti
            // landing while the card is still moving reads as glitchy.
            setTimeout(fireConfetti, 280);
        }
    });

    onDestroy(() => {
        // Stop any in-flight confetti animation when the welcome
        // dismisses, so a leftover canvas doesn't render briefly over
        // the main app shell.
        try {
            confetti.reset();
        } catch {
            // Ignore — reset is best-effort cleanup.
        }
    });

    let currentStep = $derived(STEPS[stepIndex]);
    let isFirstStep = $derived(stepIndex === 0);
    let isLastStep = $derived(stepIndex === STEPS.length - 1);
    /** Wave 7.8 (2026-05-28): "Skip intro" should only show when the
     *  user is still in the marketing/privacy steps BEFORE the pack
     *  picker. Previously the button was shown on the packs step too,
     *  but `skipToPacks` short-circuits when already at/past packs —
     *  so the button looked broken ("does nothing when pressed"). The
     *  packs step is itself the destination, so once you're there
     *  the button has no job. */
    let canSkipIntro = $derived.by(() => {
        const packsIdx = STEPS.findIndex((s) => s.kind === 'packs');
        return packsIdx > 0 && stepIndex < packsIdx;
    });

    // ─── Pack picker state (used by the final step only) ──────────
    /** Local pack selection. Initialized from current store value
     *  (default `['core']`) and flushed only on commit. */
    let selectedPacks = $state<Set<ToolPackId>>(new Set(get(enabledPackIds)));

    function isSelected(id: ToolPackId): boolean {
        return selectedPacks.has(id);
    }

    interface WelcomePack {
        id: ToolPackId;
        label: string;
        tagline: string;
        icon: typeof LucideIcon;
        color: string;
        required?: boolean;
        /** Catalog category keys whose tool count rolls up into this
         *  pack's "X tools" badge. `null` means the count is fixed
         *  (Core) — used for the always-on bundle that isn't a single
         *  `category` entry in `appScreens.ts`. */
        catalogCategories: string[] | null;
        fixedCount?: number;
    }
    // Wave 7.5 (2026-05-28): Automation pack is hidden — it's a v2
    // feature, listing it as if shipped is misleading. Pack list also
    // gained tool counts (computed from `appScreens.ts` so they can't
    // drift) and richer taglines, since the standalone catalog step
    // was merged into here.
    const WELCOME_PACKS: WelcomePack[] = [
        {
            id: 'core',
            label: 'Core',
            tagline: 'Global search, clipboard, snippets, voice, notes, privacy audit. Always enabled.',
            icon: ShieldCheck,
            color: '#10b981',
            required: true,
            catalogCategories: null,
            fixedCount: 5,
        },
        {
            id: 'utils',
            label: 'Utilities',
            tagline: 'Hashes, encoders, QR codes, password generator, unit/format converters, calculator.',
            icon: Wrench,
            color: '#f59e0b',
            catalogCategories: ['Utils'],
        },
        {
            id: 'development',
            label: 'Developer Tools',
            tagline: 'JWT decoder, regex tester, SQL formatter, diff viewer, fake-data generator, secret leak scanner, SSH key manager.',
            icon: Code,
            color: '#22c55e',
            catalogCategories: ['Development'],
        },
        {
            id: 'privacy',
            label: 'Privacy Toolkit',
            tagline: 'File shredder, screenshot redactor, privacy audit, Windows privacy hardening.',
            icon: Lock,
            color: '#ef4444',
            catalogCategories: ['Privacy'],
        },
        {
            id: 'document',
            label: 'Documents',
            tagline: 'Word, CSV, and Excel cleanup, conversion, and merging.',
            icon: FileText,
            color: '#fb7185',
            catalogCategories: ['Document'],
        },
        {
            id: 'image',
            label: 'Image Processing',
            tagline: 'Convert, compress, resize, watermark, favicon generator, OCR.',
            icon: ImageIcon,
            color: '#06b6d4',
            catalogCategories: ['Image'],
        },
        {
            id: 'media',
            label: 'Media',
            tagline: 'Record your screen, extract audio, and compress video locally.',
            icon: Video,
            color: '#f97316',
            catalogCategories: ['Media'],
        },
        {
            id: 'file',
            label: 'File Tools',
            tagline: 'Disk cleaner, duplicate finder, bulk rename, folder diff/merge.',
            icon: Files,
            color: '#64748b',
            catalogCategories: ['File'],
        },
        {
            id: 'time-focus',
            label: 'Time & Focus',
            tagline: 'Local reminders and a focus session that nudges you off distracting apps.',
            icon: Timer,
            color: '#8b5cf6',
            catalogCategories: ['Focus'],
        },
        // Automation pack is intentionally omitted in v1 (v2-only
        // feature). Listing it here would imply it's shippable now.
    ];

    /** Tool count for a pack — computed from `appScreens.ts` so the
     *  number on the welcome can never disagree with what the user
     *  sees later in Settings → Tool Packs. */
    function countToolsForPack(pack: WelcomePack): number {
        if (typeof pack.fixedCount === 'number') return pack.fixedCount;
        if (!pack.catalogCategories) return 0;
        return toolIdsByCategory(
            pack.catalogCategories as Parameters<typeof toolIdsByCategory>[0],
        ).length;
    }
    /** Total tool count surfaced in the hero — recomputed each render
     *  via $derived so it tracks Settings changes immediately. */
    let totalToolCount = $derived(toolScreens.filter((tool) => !tool.hidden).length);

    function togglePack(pack: WelcomePack) {
        if (pack.required) return;
        const next = new Set(selectedPacks);
        if (next.has(pack.id)) next.delete(pack.id);
        else next.add(pack.id);
        next.add('core');
        selectedPacks = next;
    }

    // ─── Navigation ───────────────────────────────────────────────
    function next() {
        if (stepIndex < STEPS.length - 1) {
            direction = 1;
            stepIndex++;
        }
    }
    function prev() {
        if (stepIndex > 0) {
            direction = -1;
            stepIndex--;
        }
    }
    /** Jump directly to the pack picker — for returning users who don't
     *  want to re-watch the marketing flow on a welcome reset. The pack
     *  picker is the second-to-last step (magic moment is the very
     *  last); landing on it gives the user the choice to skip
     *  remaining marketing OR continue to the confetti finale. */
    function skipToPacks() {
        const packsIdx = STEPS.findIndex((s) => s.kind === 'packs');
        if (packsIdx < 0 || stepIndex >= packsIdx) return;
        direction = 1;
        stepIndex = packsIdx;
    }

    async function enterApp() {
        if (entering) return;
        entering = true;
        try {
            await saveInitialToolPackSelection([...selectedPacks]);
            // Wave 7.5 (2026-05-28): suppress the old 6-step TourModal
            // from auto-opening on Home — Welcome 2.0 already covered
            // every topic the tour walked through (pillars, hotkey,
            // privacy, pin/folder tips). The TourModal component stays
            // in the codebase; users who want to replay it can via
            // Settings → Onboarding → "Replay tour", which flips this
            // same flag back to false. UxAudit UX-A-01 fix.
            settings.update((s) => ({ ...s, tourCompleted: true }));
            await completeWelcome();
            // Wave 7.9 (2026-05-28): now that welcome lives in its own
            // Tauri window, completing the flow means closing the
            // welcome window and revealing main. The `welcome_finished`
            // Rust command also emits `kit-state-refresh` so main re-
            // reads onboarding / packs / settings from disk before it
            // becomes visible — that way the pack selections the user
            // just made are reflected in main's first paint. We invoke
            // this UNCONDITIONALLY: if for some reason welcome is being
            // rendered inline (fallback path that no longer exists in
            // v1 — kept defensively), Rust just no-ops when the window
            // doesn't exist.
            try {
                await invoke('welcome_finished');
            } catch (handoffError) {
                // Non-fatal — completeWelcome already persisted. If the
                // handoff fails the user can quit + relaunch and the
                // welcome window won't be re-created.
                console.warn('welcome_finished handoff failed:', handoffError);
            }
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            toast(
                `Setup couldn't save: ${message}. You can change packs later in Settings.`,
                'error',
                6000,
            );
            try {
                // Still suppress the tour in the fallback path so a
                // partial save doesn't accidentally surface it.
                settings.update((s) => ({ ...s, tourCompleted: true }));
                await completeWelcome();
                try {
                    await invoke('welcome_finished');
                } catch {
                    /* see above — non-fatal */
                }
            } catch {
                /* completeWelcome already has its own local fallback */
            }
        } finally {
            entering = false;
        }
    }

    // ─── macOS-style slide transition ─────────────────────────────
    // What makes macOS slides feel "natural" and different from a
    // generic Svelte fly:
    //
    //   1. No fade. Opacity stays at 1 the entire transition. macOS
    //      slides are pure spatial — one view pushes the other off
    //      to the side at the same speed, like a physical card.
    //
    //   2. Full-width offset (not a 44px hint). The leaving element
    //      exits beyond the container's edge; the new one enters from
    //      the opposite edge. We use `translateX(±100%)` so it always
    //      matches the container's actual width regardless of viewport.
    //
    //   3. Decelerating easing (quintOut). Apple's standard slide
    //      curve is a strong out-easing — quick start, gentle settle.
    //      cubicOut is too gentle, quintOut nails the "release a
    //      sprung card" feel.
    //
    //   4. ~460ms duration. Apple's own view-transition range is
    //      350-500ms; 460 sits in the upper half of that, which reads
    //      as "deliberate, considered" rather than "fast snap" — closer
    //      to the iOS settings-app slide feel than a tab swap.
    //
    //   5. translate3d (not translate) so the browser keeps each
    //      transitioning element on its own GPU layer.
    //
    // For prefers-reduced-motion users, we fall back to an opacity-only
    // fade with no spatial movement — passing x=0 routes through the
    // fade branch of the function.
    function macSlide(
        _node: Element,
        {
            x = 100,
            duration = 460,
            easing = quintOut,
        }: { x?: number; duration?: number; easing?: (t: number) => number } = {},
    ): TransitionConfig {
        if (x === 0) {
            // Reduced-motion path: just fade. No spatial movement.
            return {
                duration,
                easing,
                css: (t) => `opacity: ${t};`,
            };
        }
        // Standard macOS-style slide. Opacity stays 1 — only translate.
        return {
            duration,
            easing,
            css: (_t, u) => `transform: translate3d(${u * x}%, 0, 0);`,
        };
    }

    const TRANSITION_DURATION = 460;
    const REDUCED_MOTION_DURATION = 140;
    /** In x: direction*100% means "comes from the side the user is
     *  moving toward" — forward (direction=1) brings the new step in
     *  from the right (x=+100%), back brings it in from the left. */
    let inX = $derived(direction * 100);
    /** Out x: the inverse — old step leaves in the OPPOSITE direction
     *  to where the new step is arriving. Forward: out to the left
     *  (-100%). Back: out to the right (+100%). */
    let outX = $derived(-direction * 100);
</script>

<div class="welcome">
    <!-- Subtle skip control. Wave 7.8 (2026-05-28): now gated on
         `canSkipIntro` (false once we're at/past the pack picker)
         so the button never appears in a state where pressing it
         does nothing. -->
    {#if canSkipIntro}
        <button
            type="button"
            class="skip-link"
            onclick={skipToPacks}
            title="Skip the intro and jump to pack selection"
        >
            Skip intro →
        </button>
    {/if}

    <div class="welcome-inner">
        <!-- Step indicator dots — non-interactive, position only.
             Filled = current; outlined small = others. -->
        <div class="step-dots" aria-hidden="true">
            {#each STEPS as _, i (i)}
                <span
                    class="dot"
                    class:is-current={i === stepIndex}
                    class:is-past={i < stepIndex}
                ></span>
            {/each}
        </div>

        <!-- Step stack — both step-wraps live in the same grid cell so
             they overlay during a transition. min-height stabilizes the
             cell at the larger step's size so going Back doesn't shrink
             the cell under a finishing animation. -->
        <div class="step-stack">
            {#key stepIndex}
                <div
                    class="step-wrap"
                    class:is-marketing={currentStep.kind === 'intro' || currentStep.kind === 'pillar' || currentStep.kind === 'magic'}
                    class:is-packs={currentStep.kind === 'packs'}
                    class:is-catalog={currentStep.kind === 'catalog'}
                    class:is-privacy={currentStep.kind === 'privacy'}
                    in:macSlide={{
                        x: prefersReducedMotion ? 0 : inX,
                        duration: prefersReducedMotion
                            ? REDUCED_MOTION_DURATION
                            : TRANSITION_DURATION,
                    }}
                    out:macSlide={{
                        x: prefersReducedMotion ? 0 : outX,
                        duration: prefersReducedMotion
                            ? REDUCED_MOTION_DURATION
                            : TRANSITION_DURATION,
                    }}
                >
                    {#if currentStep.kind === 'intro' || currentStep.kind === 'pillar'}
                        <!-- Marketing step layout — hero + CTA row.
                             `<currentStep.icon />` is Svelte 5's dynamic
                             component syntax via member access; the icon
                             is already imported synchronously above. -->
                        {@const StepIcon = currentStep.icon}
                        <div class="hero" style="--accent: {currentStep.color};">
                            <div class="hero-icon" aria-hidden="true">
                                <StepIcon class="hero-glyph" />
                            </div>
                            <h1 class="hero-title">{currentStep.title}</h1>
                            <p class="hero-sub">{currentStep.subtitle}</p>
                        </div>
                        <div class="actions">
                            {#if !isFirstStep}
                                <button type="button" class="back-link" onclick={prev}>
                                    <ArrowLeft class="back-arrow" />
                                    <span>Back</span>
                                </button>
                            {:else}
                                <span class="back-spacer"></span>
                            {/if}
                            <button
                                type="button"
                                class="primary-cta"
                                style="--accent: {currentStep.color};"
                                onclick={next}
                            >
                                <span>{currentStep.ctaLabel ?? 'Continue'}</span>
                                <ArrowRight class="cta-arrow" />
                            </button>
                        </div>
                    {:else if currentStep.kind === 'privacy'}
                        <!-- Wave 7 (2026-05-28): Privacy promise step.
                             Three concrete rules in big, scannable
                             cards instead of a wall of paragraph text.
                             Each rule maps to a real architectural
                             constraint enforced by the code — not
                             aspirational marketing. -->
                        {@const PrivIcon = currentStep.icon}
                        <div class="hero hero-compact" style="--accent: {currentStep.color};">
                            <div class="hero-icon hero-icon-compact" aria-hidden="true">
                                <PrivIcon class="hero-glyph" />
                            </div>
                            <h1 class="hero-title hero-title-compact">{currentStep.title}</h1>
                            <p class="hero-sub hero-sub-compact">{currentStep.subtitle}</p>
                        </div>

                        <div class="privacy-grid">
                            <div class="privacy-card" style="--accent: #10b981;">
                                <div class="privacy-card-icon" aria-hidden="true">
                                    <WifiOff class="privacy-card-ico" />
                                </div>
                                <h3 class="privacy-card-title">No cloud uploads</h3>
                                <p class="privacy-card-desc">
                                    Search indexes, clipboard history, voice transcripts,
                                    notes — everything is stored on this machine. The app
                                    works fully offline.
                                </p>
                            </div>
                            <div class="privacy-card" style="--accent: #a855f7;">
                                <div class="privacy-card-icon" aria-hidden="true">
                                    <UserX class="privacy-card-ico" />
                                </div>
                                <h3 class="privacy-card-title">No account, ever</h3>
                                <p class="privacy-card-desc">
                                    No sign-up, no email collection, no telemetry. You don't
                                    have an account because there's no server to have one on.
                                </p>
                            </div>
                            <div class="privacy-card" style="--accent: #06b6d4;">
                                <div class="privacy-card-icon" aria-hidden="true">
                                    <EyeOff class="privacy-card-ico" />
                                </div>
                                <h3 class="privacy-card-title">No tracking</h3>
                                <p class="privacy-card-desc">
                                    No analytics SDK, no error-reporting service, no
                                    "anonymous usage" toggles. We literally don't know
                                    you exist.
                                </p>
                            </div>
                        </div>

                        <div class="actions">
                            <button type="button" class="back-link" onclick={prev}>
                                <ArrowLeft class="back-arrow" />
                                <span>Back</span>
                            </button>
                            <button
                                type="button"
                                class="primary-cta"
                                style="--accent: {currentStep.color};"
                                onclick={next}
                            >
                                <span>{currentStep.ctaLabel ?? 'Continue'}</span>
                                <ArrowRight class="cta-arrow" />
                            </button>
                        </div>
                    {:else if currentStep.kind === 'magic'}
                        <!-- Wave 7 (2026-05-28): Magic-moment finale.
                             Confetti fires on entry (see fireConfetti
                             effect). Big chord glyph + the actual
                             keystroke the user needs to remember. This
                             is the moment UxAudit UX-A-01 called out
                             as missing from the original wizard —
                             "end on the action, not the pack picker." -->
                        {@const MagicIcon = currentStep.icon}
                        <div class="hero magic-hero" style="--accent: {currentStep.color};">
                            <div class="hero-icon" aria-hidden="true">
                                <MagicIcon class="hero-glyph" />
                            </div>
                            <h1 class="hero-title">{currentStep.title}</h1>
                            <p class="hero-sub">{currentStep.subtitle}</p>

                            <!-- The chord. The whole step's point. -->
                            <div class="magic-chord" aria-hidden="true">
                                <kbd class="magic-key">Ctrl</kbd>
                                <span class="magic-plus">+</span>
                                <kbd class="magic-key">Alt</kbd>
                                <span class="magic-plus">+</span>
                                <kbd class="magic-key">K</kbd>
                            </div>
                            <p class="magic-hint">Try it after you close this window.</p>
                        </div>
                        <div class="actions">
                            <button type="button" class="back-link" onclick={prev}>
                                <ArrowLeft class="back-arrow" />
                                <span>Back</span>
                            </button>
                            <button
                                type="button"
                                class="primary-cta primary-cta-magic"
                                style="--accent: {currentStep.color};"
                                onclick={enterApp}
                                disabled={entering}
                            >
                                <span>{entering ? 'Setting up…' : (currentStep.ctaLabel ?? 'Enter KeepItLocal')}</span>
                                <ArrowRight class="cta-arrow" />
                            </button>
                        </div>
                    {:else if currentStep.kind === 'packs'}
                        <!-- Wave 7.5 (2026-05-28): merged catalog tour +
                             pack picker. Each pack card now carries
                             its tagline + a live tool count (from
                             appScreens.ts), so the user sees WHAT they're
                             picking, not just the name. -->
                        {@const PacksIcon = currentStep.icon}
                        <div class="hero hero-compact" style="--accent: {currentStep.color};">
                            <div class="hero-icon hero-icon-compact" aria-hidden="true">
                                <PacksIcon class="hero-glyph" />
                            </div>
                            <h1 class="hero-title hero-title-compact">{currentStep.title}</h1>
                            <p class="hero-sub hero-sub-compact">
                                {currentStep.subtitle}
                                <span class="catalog-count-pill">{totalToolCount}+ tools</span>
                            </p>
                        </div>

                        <div class="pack-grid">
                            {#each WELCOME_PACKS as pack (pack.id)}
                                {@const PackIcon = pack.icon}
                                {@const selected = isSelected(pack.id)}
                                {@const toolCount = countToolsForPack(pack)}
                                <label
                                    class="pack-pick"
                                    class:is-selected={selected}
                                    class:is-required={pack.required}
                                    style="--pack-color: {pack.color};"
                                    title={pack.required
                                        ? `${pack.label} — required, always enabled`
                                        : selected
                                          ? `${pack.label} — click to disable`
                                          : `${pack.label} — click to enable`}
                                >
                                    <input
                                        type="checkbox"
                                        class="pack-pick-input"
                                        checked={selected}
                                        disabled={pack.required}
                                        onchange={() => togglePack(pack)}
                                    />
                                    <div class="pack-pick-icon" aria-hidden="true">
                                        <PackIcon class="pack-pick-ico" />
                                    </div>
                                    <div class="pack-pick-text">
                                        <div class="pack-pick-row">
                                            <span class="pack-pick-name">{pack.label}</span>
                                            {#if pack.required}
                                                <span class="chip-required">ALWAYS ON</span>
                                            {:else}
                                                <span class="pack-pick-count">
                                                    {toolCount} tool{toolCount === 1 ? '' : 's'}
                                                </span>
                                            {/if}
                                        </div>
                                        <p class="pack-pick-tag">{pack.tagline}</p>
                                    </div>
                                    <div class="pack-pick-check" aria-hidden="true">
                                        {#if selected}
                                            <Check class="check-ico" />
                                        {/if}
                                    </div>
                                </label>
                            {/each}
                        </div>

                        <div class="actions">
                            <button type="button" class="back-link" onclick={prev}>
                                <ArrowLeft class="back-arrow" />
                                <span>Back</span>
                            </button>
                            <button
                                type="button"
                                class="primary-cta"
                                style="--accent: {currentStep.color};"
                                onclick={next}
                                disabled={entering}
                            >
                                <span>{currentStep.ctaLabel ?? 'Continue'}</span>
                                <ArrowRight class="cta-arrow" />
                            </button>
                        </div>
                    {/if}
                </div>
            {/key}
        </div>
    </div>
</div>

<style>
    /* No welcome-level scroll. The welcome occupies the entire viewport
       and inner layout uses flex to claim every pixel intentionally.
       What COULD overflow (the pack-grid in step 5) scrolls INTERNALLY
       so the hero + action row stay pinned in place. That's what makes
       the buttons land at the same Y on every step — the action row is
       anchored to the bottom of the step-stack, not pushed down by the
       content above it. */
    .welcome {
        position: relative;
        height: 100vh;
        width: 100vw;
        background: var(--color-bg);
        color: var(--color-text);
        /* Wave 7.8.2 (2026-05-28): switched from `overflow: hidden` to
           `overflow-x: clip; overflow-y: hidden`. We still clip
           horizontally for the slide transitions, but giving the y axis
           its own value lets the inner padding-bottom buffer absorb
           shadow render without producing the visible "seam" that hard
           overflow:hidden would create at the welcome→step-stack
           handoff. */
        overflow-x: clip;
        overflow-y: hidden;
        /* Wave 7.8.2: dropped `align-items: center` (vertical
           centering). The welcome-inner now stretches to fill the full
           viewport content area top-to-bottom — content flows naturally
           from the top, actions sit at the bottom, and shadows below
           the actions row have welcome's padding-bottom (72px) to
           render into instead of being clipped by step-stack at the
           welcome-inner's centered bottom edge. */
        display: flex;
        justify-content: center;
        padding: 32px 28px 72px;
    }
    .welcome-inner {
        width: 100%;
        max-width: 980px;
        /* Wave 7.8.2 (2026-05-28): dropped `max-height` cap + kept
           `height: 100%` so welcome-inner spans the entire welcome
           content area. No more centered-card-in-the-middle layout —
           content fills the viewport top to bottom. */
        height: 100%;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 24px;
    }

    /* ─── Skip link ─────────────────────────────────────────────── */
    /* Top-right corner; small + subtle so it never competes with the
       primary CTA but is always reachable for returning users who
       reset welcome. */
    .skip-link {
        position: absolute;
        top: 20px;
        right: 24px;
        background: transparent;
        border: 1px solid transparent;
        color: var(--color-muted);
        font-size: 12px;
        font-weight: 500;
        padding: 6px 10px;
        border-radius: 8px;
        cursor: pointer;
        transition:
            color var(--dur-micro, 130ms) var(--ease-out, ease),
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .skip-link:hover {
        color: var(--color-text);
        background: var(--color-panel-2);
        border-color: var(--color-border);
    }

    /* ─── Step indicator dots ──────────────────────────────────── */
    .step-dots {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        height: 14px;
    }
    .dot {
        width: 6px;
        height: 6px;
        border-radius: 50%;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            width var(--dur-micro, 130ms) var(--ease-out, ease),
            border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .dot.is-past {
        background: color-mix(in srgb, var(--color-accent) 30%, var(--color-panel-2));
        border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
    }
    .dot.is-current {
        width: 20px;
        border-radius: 4px;
        background: var(--color-accent);
        border-color: var(--color-accent);
    }

    /* ─── Step stack ────────────────────────────────────────────── */
    /* The flex chain here is load-bearing — it's what makes the
       action row stay anchored to the same Y across steps AND lets
       the pack step's grid scroll internally instead of pushing
       Enter off-screen.

       Three properties that all have to be right together:

         1. `flex: 1 1 0` (not just `flex: 1`) — explicit `0` basis
            ensures step-stack starts from zero and grows ONLY into
            the space welcome-inner allocates it. Without the explicit
            basis, the grid contents can claim a natural min-size that
            prevents step-stack from shrinking back to its share.

         2. `grid-template-rows: 1fr` — the grid row has a definite
            height (1fr of step-stack's definite flex height). step-wrap
            inside resolves `height: 100%` against THAT, gets a
            definite height, and can finally distribute its own flex
            children correctly. Without 1fr the row is `auto` and the
            chain breaks.

         3. `overflow: hidden` (NOT `overflow-x: clip`) — this is the
            piece I had wrong. `clip` doesn't establish a containment
            block for vertical overflow, so step-wrap's content was
            rendering beyond step-stack's allocated height. With
            `overflow: hidden`, the container actually forces children
            to live inside its bounds → pack-grid's internal
            overflow-y:auto engages → action row stays put.

         The horizontal slide still works correctly because the macSlide
         transition uses `translate3d` (transform-based, not layout-
         based), and overflow: hidden naturally clips translated content
         to the container — that's the same containment that gives the
         "card pushing card off-screen" effect. */
    .step-stack {
        position: relative;
        width: 100%;
        flex: 1 1 0;
        min-height: 0;
        /* Wave 7.8.2 (2026-05-28): split overflow per-axis. We still
           need to clip horizontally so the slide-out step doesn't
           render outside welcome-inner during transitions. But the
           Y axis is now `visible` so the primary-CTA's box-shadow
           glow can extend below step-stack's bottom into welcome's
           padding-bottom buffer — eliminates the "thin black seam"
           between the shadow and the background that hard clip
           was creating.

           `overflow-x: clip` (not `hidden`) is required here so the
           spec's "if one axis is hidden and the other visible, the
           visible axis is forced to auto" quirk doesn't kick in.
           `clip` clips without creating a scroll container, leaving
           overflow-y as truly visible. Supported in Chromium 90+
           which WebView2 satisfies. */
        overflow-x: clip;
        overflow-y: visible;
        display: grid;
        grid-template-areas: 'step';
        grid-template-rows: 1fr;
    }
    /* step-wrap fills the entire step-stack cell so all internal
       layout (hero, actions, optional pack grid) anchors to its
       edges, not to the natural content height. */
    .step-wrap {
        grid-area: step;
        width: 100%;
        height: 100%;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 24px;
        /* Wave 7.8.2 (2026-05-28): step-wrap bottom padding reduced
           from 56 → 32 px. The original 56 was needed because
           step-stack had `overflow: hidden` and clipped the CTA's
           box-shadow; now that step-stack uses
           `overflow-x: clip; overflow-y: visible`, the shadow renders
           freely into welcome's padding-bottom buffer (72 px) and the
           extra step-wrap buffer is no longer load-bearing. 32 px
           keeps comfortable breathing room without leaving a big
           empty gap between the actions row and the visual bottom. */
        padding: 24px 0 32px;
        /* GPU layer hints — paint each step on its own compositor
           layer so transform stays smooth. */
        will-change: transform;
        backface-visibility: hidden;
        transform: translateZ(0);
    }

    /* ─── Marketing layout (steps 1-4) ─────────────────────────── */
    /* hero gets auto margins on top AND bottom so it visually centers
       in the available space, while actions stays anchored to the
       bottom edge of step-wrap. Result: hero floats in the middle,
       actions sit at the same Y as every other step. */
    .step-wrap.is-marketing .hero {
        margin-top: auto;
        margin-bottom: auto;
    }

    /* ─── Pack picker layout (step 5) ──────────────────────────── */
    /* Hero pins to the top (no auto margins). pack-grid is the only
       element that flexes — it takes all remaining space between hero
       and actions, and SCROLLS INTERNALLY when there are more packs
       than fit. This is the fix for "scroll hides the hero": only the
       pack list scrolls, hero and actions stay visible. */
    /* Wave 7.8.1 (2026-05-28): hero capped at a max-height + allowed
       to shrink rather than `flex-shrink: 0`. On smaller windows where
       the subtitle wraps to multiple lines, the previous flex-shrink:0
       let the hero grow until it pushed the actions row past
       step-stack's overflow:hidden boundary — the actions div ended
       up cut in half. Now the hero is bounded; pack-grid still claims
       all remaining space and scrolls internally, but the actions row
       is guaranteed to stay in view. */
    .step-wrap.is-packs .hero {
        flex-shrink: 0;
        max-height: 32%;
    }
    .step-wrap.is-packs .pack-grid {
        flex: 1;
        min-height: 0;
        width: 100%;
        overflow-y: auto;
        /* Tiny padding-right so the optional scrollbar doesn't kiss
           the pack-card right edges when it appears. */
        padding-right: 4px;
        /* Custom thin scrollbar — Chromium / WebView2. */
        scrollbar-width: thin;
        scrollbar-color: var(--color-border) transparent;
    }
    .step-wrap.is-packs .pack-grid::-webkit-scrollbar {
        width: 6px;
    }
    .step-wrap.is-packs .pack-grid::-webkit-scrollbar-thumb {
        background: var(--color-border);
        border-radius: 3px;
    }
    .step-wrap.is-packs .actions {
        flex-shrink: 0;
    }

    /* Wave 7.5 (2026-05-28): the standalone catalog-grid + catalog-tile
       CSS was removed when the catalog step was merged into the pack
       picker. The .catalog-count-pill class remains — it surfaces the
       total tool count in the merged hero. */
    .catalog-count-pill {
        display: inline-block;
        margin-left: 8px;
        padding: 1px 8px;
        background: color-mix(in srgb, var(--accent) 16%, transparent);
        border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
        color: var(--accent);
        font-size: 11.5px;
        font-weight: 600;
        border-radius: 999px;
        vertical-align: middle;
    }

    /* ─── Privacy promise layout (Wave 7, 2026-05-28) ──────────── */
    /* Three big concrete-rule cards stacked horizontally on wide
       screens, vertically on narrow. Each card is its own color so
       the three rules read as distinct guarantees, not one paragraph. */
    .step-wrap.is-privacy .hero,
    .step-wrap.is-privacy .actions {
        flex-shrink: 0;
    }
    .step-wrap.is-privacy .privacy-grid {
        flex: 1 1 0;
        min-height: 0;
        width: 100%;
        max-width: 900px;
        margin: 0 auto;
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
        gap: 16px;
        align-content: center;
        padding: 4px;
        overflow-y: auto;
    }
    .privacy-card {
        display: flex;
        flex-direction: column;
        gap: 10px;
        padding: 20px 18px;
        background: color-mix(in srgb, var(--accent) 5%, var(--color-panel));
        border: 1px solid color-mix(in srgb, var(--accent) 30%, var(--color-border));
        border-radius: 16px;
    }
    .privacy-card-icon {
        width: 44px;
        height: 44px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: color-mix(in srgb, var(--accent) 18%, transparent);
        border-radius: 12px;
        color: var(--accent);
    }
    .step-wrap.is-privacy :global(.privacy-card-ico) {
        width: 22px;
        height: 22px;
    }
    .privacy-card-title {
        margin: 0;
        font-size: 15px;
        font-weight: 600;
        color: var(--color-text);
        letter-spacing: -0.005em;
    }
    .privacy-card-desc {
        margin: 0;
        font-size: 12.5px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }

    /* ─── Magic-moment finale (Wave 7, 2026-05-28) ─────────────── */
    /* The Ctrl + Alt + K chord rendered BIG. This is the literal
       payoff — the keystroke the user needs to remember. Each <kbd>
       is a 3D-ish key cap with depth shadow + accent border. */
    .magic-hero {
        gap: 22px !important;
        /* Wave 7.8.3 (2026-05-28): override the .is-marketing auto
           centering for THIS step only. The packs step pins its hero
           to the top of step-wrap; the magic step inherited the
           marketing layout's `margin: auto` which centered the hero
           vertically. Result: transitioning packs → magic (or back)
           showed the hero contents jumping ~200 px up/down because
           the two layouts positioned the hero at different Y values.
           Anchoring magic-hero to the top (with auto on the bottom
           so actions stays pinned at the welcome-inner's bottom)
           matches the packs hero position — no visible Y-jump during
           the horizontal slide. The chord is still impactful at the
           top; it doesn't need to be vertically centered to read as
           the headline moment. */
        margin-top: 0 !important;
        margin-bottom: auto !important;
    }
    .magic-chord {
        margin-top: 12px;
        display: inline-flex;
        align-items: center;
        gap: 10px;
        font-variant-numeric: tabular-nums;
    }
    .magic-key {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 56px;
        height: 56px;
        padding: 0 18px;
        background: var(--color-panel);
        border: 1px solid color-mix(in srgb, var(--accent) 50%, var(--color-border));
        border-bottom-width: 3px;
        border-bottom-color: color-mix(in srgb, var(--accent) 70%, var(--color-border));
        border-radius: 10px;
        font-family: 'Inter', system-ui, sans-serif;
        font-size: 18px;
        font-weight: 600;
        color: var(--color-text);
        box-shadow:
            0 2px 0 0 color-mix(in srgb, var(--accent) 40%, var(--color-border)),
            0 4px 12px -4px color-mix(in srgb, var(--accent) 30%, transparent);
        letter-spacing: 0.02em;
    }
    .magic-plus {
        font-size: 22px;
        color: var(--color-muted);
        font-weight: 300;
    }
    .magic-hint {
        margin-top: 4px;
        font-size: 12.5px;
        color: var(--color-muted);
        font-style: italic;
    }
    .primary-cta-magic {
        /* The finale CTA gets a little extra glow so it visually
           reads as the "celebrate" button, not just another Continue. */
        box-shadow:
            0 0 0 0 transparent,
            0 8px 24px -4px color-mix(in srgb, var(--accent) 40%, transparent);
    }

    /* ─── Hero (intro + pillar) ────────────────────────────────── */
    .hero {
        display: flex;
        flex-direction: column;
        align-items: center;
        text-align: center;
        gap: 18px;
        max-width: 640px;
    }
    .hero-compact {
        gap: 12px;
        max-width: 560px;
    }
    /* Glow is BOX-SHADOW (cheap, GPU-friendly) instead of a separate
       filter:blur element. Same look, no blur filter recomputing on
       every animation frame. */
    .hero-icon {
        position: relative;
        width: 76px;
        height: 76px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border-radius: 18px;
        background: color-mix(in srgb, var(--accent) 14%, var(--color-panel));
        border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--color-border));
        color: var(--accent);
        box-shadow:
            0 0 0 1px color-mix(in srgb, var(--accent) 18%, transparent),
            0 12px 56px color-mix(in srgb, var(--accent) 32%, transparent);
    }
    .hero-icon-compact {
        width: 52px;
        height: 52px;
        border-radius: 14px;
        box-shadow:
            0 0 0 1px color-mix(in srgb, var(--accent) 18%, transparent),
            0 8px 36px color-mix(in srgb, var(--accent) 28%, transparent);
    }
    .welcome :global(.hero-glyph) {
        width: 32px;
        height: 32px;
    }
    .hero-icon-compact :global(.hero-glyph) {
        width: 22px;
        height: 22px;
    }
    .hero-title {
        margin: 0;
        font-size: 36px;
        font-weight: 700;
        letter-spacing: -0.025em;
        line-height: 1.12;
        color: var(--color-text);
        text-wrap: balance;
    }
    .hero-title-compact {
        font-size: 26px;
    }
    .hero-sub {
        margin: 0;
        font-size: 14.5px;
        line-height: 1.6;
        color: var(--color-text-secondary);
        max-width: 560px;
        text-wrap: pretty;
    }
    .hero-sub-compact {
        font-size: 13.5px;
    }

    /* ─── Actions row ──────────────────────────────────────────── */
    .actions {
        width: 100%;
        max-width: 640px;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
    }
    .back-spacer {
        /* invisible placeholder so the primary CTA stays right-aligned
           on the first step (no Back button there) */
        width: 64px;
    }
    .back-link {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 8px 12px;
        background: transparent;
        border: none;
        color: var(--color-text-secondary);
        font-size: 13px;
        font-weight: 500;
        cursor: pointer;
        border-radius: 8px;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .back-link:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .welcome :global(.back-arrow) {
        width: 14px;
        height: 14px;
    }

    /* ─── Primary CTA ──────────────────────────────────────────── */
    .primary-cta {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        padding: 14px 26px;
        font-size: 14.5px;
        font-weight: 600;
        color: var(--color-accent-contrast, #03110a);
        background: var(--accent);
        border: none;
        border-radius: 999px;
        cursor: pointer;
        box-shadow:
            0 0 0 1px color-mix(in srgb, var(--accent) 60%, transparent),
            0 8px 24px color-mix(in srgb, var(--accent) 28%, transparent);
        transition:
            transform var(--dur-micro, 130ms) var(--ease-out, ease),
            box-shadow var(--dur-micro, 130ms) var(--ease-out, ease),
            background-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .primary-cta:hover:not(:disabled) {
        background: color-mix(in srgb, var(--accent) 92%, white);
        box-shadow:
            0 0 0 1px color-mix(in srgb, var(--accent) 75%, transparent),
            0 10px 32px color-mix(in srgb, var(--accent) 36%, transparent);
        transform: translateY(-1px);
    }
    .primary-cta:focus-visible {
        outline: none;
        box-shadow:
            0 0 0 3px color-mix(in srgb, var(--accent) 35%, transparent),
            0 8px 24px color-mix(in srgb, var(--accent) 28%, transparent);
    }
    .primary-cta:disabled {
        opacity: 0.7;
        cursor: progress;
    }
    .welcome :global(.cta-arrow) {
        width: 16px;
        height: 16px;
    }

    /* ─── Pack picker grid (final step) — compact row chips ──────
       Each pack is now a single-line row (~46px tall) showing just
       icon + name + optional REQUIRED chip + check. 10 packs in a
       3-col grid = 4 rows × 46px + 3 gaps × 8px = 208px total. Fits
       comfortably between hero (150px) and actions (50px) in every
       viewport ≥ 600px tall, no internal scroll needed. The longer
       descriptions still live in Tool Packs page after install. */
    .pack-grid {
        width: 100%;
        display: grid;
        /* Wave 7.5 (2026-05-28): wider cards now that they carry a
           tagline + tool count. Two columns on most screens; auto-
           fills wider on bigger windows. */
        grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
        gap: 10px;
    }
    .pack-pick {
        position: relative;
        display: flex;
        align-items: flex-start;
        gap: 12px;
        padding: 12px 14px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 12px;
        cursor: pointer;
        transition:
            border-color var(--dur-micro, 130ms) var(--ease-out, ease),
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            box-shadow var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .pack-pick:hover:not(.is-required) {
        background: var(--color-panel-2);
    }
    .pack-pick.is-selected {
        border-color: color-mix(in srgb, var(--pack-color) 55%, var(--color-border));
        box-shadow: 0 0 0 1px color-mix(in srgb, var(--pack-color) 30%, transparent);
    }
    .pack-pick.is-required {
        cursor: default;
        border-color: color-mix(in srgb, var(--pack-color) 55%, var(--color-border));
        box-shadow: 0 0 0 1px color-mix(in srgb, var(--pack-color) 30%, transparent);
    }
    .pack-pick-input {
        position: absolute;
        opacity: 0;
        pointer-events: none;
    }
    .pack-pick-icon {
        flex: none;
        width: 36px;
        height: 36px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border-radius: 10px;
        background: color-mix(in srgb, var(--pack-color) 14%, transparent);
        color: var(--pack-color);
        margin-top: 1px;
    }
    .welcome :global(.pack-pick-ico) {
        width: 18px;
        height: 18px;
    }
    /* Wave 7.5 (2026-05-28): pack-pick-text wraps the name row + the
       tagline. flex:1 so it claims all horizontal space between the
       icon and the trailing check. */
    .pack-pick-text {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 3px;
    }
    .pack-pick-row {
        display: flex;
        align-items: baseline;
        gap: 8px;
    }
    .pack-pick-name {
        font-size: 13.5px;
        font-weight: 600;
        color: var(--color-text);
        letter-spacing: -0.008em;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .pack-pick-count {
        font-size: 10.5px;
        font-weight: 500;
        font-variant-numeric: tabular-nums;
        color: var(--color-text-secondary);
        padding: 1px 6px;
        background: color-mix(in srgb, var(--pack-color) 12%, transparent);
        border-radius: 999px;
        white-space: nowrap;
    }
    .pack-pick-tag {
        margin: 0;
        font-size: 12px;
        line-height: 1.4;
        color: var(--color-text-secondary);
    }
    .chip-required {
        font-size: 9.5px;
        font-weight: 700;
        letter-spacing: 0.08em;
        color: var(--pack-color);
        padding: 1px 6px;
        background: color-mix(in srgb, var(--pack-color) 14%, transparent);
        border-radius: 999px;
        white-space: nowrap;
    }
    .pack-pick-check {
        flex: none;
        width: 22px;
        height: 22px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border-radius: 50%;
        color: var(--pack-color);
        margin-top: 4px;
    }
    .welcome :global(.check-ico) {
        width: 14px;
        height: 14px;
    }
</style>
