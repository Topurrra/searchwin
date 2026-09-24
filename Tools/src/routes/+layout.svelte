<script lang="ts">
    import '../styles.css';
    import { onMount, onDestroy } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
    import { initSettingsStore, settings, applyFont } from '$lib/stores/settings';
    import { enabledPackIds, initToolPacksStore } from '$lib/stores/toolPacks';
    import { get } from 'svelte/store';
    import GlobalErrorScreen from '$lib/components/GlobalErrorScreen.svelte';
    import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
    import { setupI18n } from '$lib/i18n';

    // i18n bootstrap runs synchronously at module-eval time so the very
    // first paint already has strings. The UI ships English-only (the
    // Georgian translation bundle + language picker were removed), so we
    // pin the locale to 'en'. (Georgian OCR + voice are unaffected — voice
    // derives its language from the Vosk model, not the UI locale.)
    setupI18n('en'); // English-only UI

    /** Cleanup hook for the voice-session visibility watcher. Set
     *  inside the async onMount; fired from onDestroy. Lives at the
     *  component-instance scope so onDestroy can reach it after
     *  onMount's promise has settled. */
    let voiceVisibilityCleanup: (() => void) | undefined;

    /** Cleanup hook for the global push-to-talk event listeners.
     *  Mirrors voiceVisibilityCleanup — set inside the async onMount,
     *  fired from onDestroy. PTT runs in the main window only. */
    let pushToTalkCleanup: (() => void) | undefined;

    /** Cleanup hook for the command-mode coordinator. Like PTT, the
     *  coordinator runs in the main window only — it owns the
     *  continuous command session and the command matcher. */
    let commandModeCleanup: (() => void) | undefined;

    /** Cleanup hook for the global error-capture listeners (mirrors
     *  voiceVisibilityCleanup's pattern). The capture lives for the
     *  lifetime of the webview in normal operation; this hook only
     *  fires on hot-reload or programmatic teardown. */
    let errorCaptureCleanup: (() => void) | undefined;

    /** Cleanup hook for user-authored voice commands (#21). Unlike the
     *  hooks above, this runs in every window that matches voice
     *  commands — the main window, the search overlay, and the voice
     *  overlay — so each has the user's commands in its registry. */
    let userVoiceCommandsCleanup: (() => void) | undefined;
    let commandOverridesCleanup: (() => void) | undefined;

    /** Cleanup hook for the opt-in automatic Privacy Audit scheduler. Main
     *  window only — it owns the single long-lived timer + settings
     *  subscription. No-op until the user opts into a schedule in Settings. */
    let privacyAuditSchedulerCleanup: (() => void) | undefined;

    /** Cleanup hook for the global screen-recording start/stop hotkey listener.
     *  Main window only — it owns the recorder state, so it can toggle start/stop
     *  from any page. (Stop-while-minimized is handled by the floating toolbar.) */
    let screenRecToggleCleanup: (() => void) | undefined;

    let { children } = $props();

    /** Captured by `window.onerror` / `onunhandledrejection`. Async errors
     * and event-listener errors fall outside Svelte's render boundary, so
     * we route them into the same fallback UI manually. `null` means "no
     * error currently in flight". The render-time errors caught by
     * <svelte:boundary> below pass through its `failed` snippet directly
     * — no extra state needed for those. */
    let asyncError = $state<unknown>(null);

    /** Non-fatal backend init warnings (hotkey registration failures,
     *  clipboard listener errors). Shown as dismissable banners at the
     *  top of the main window. Each entry is a human-readable string. */
    let initWarnings = $state<string[]>([]);

    /** True if the preferences DB was auto-recovered from corruption on
     *  this launch. Shown as a one-time dismissable banner. */
    let dbCorruptionRecovered = $state(false);

    onMount(async () => {
        const currentLabel = getCurrentWebviewWindow().label;
        document.documentElement.dataset.windowLabel = currentLabel;

        // Voice session safety: when the main window goes hidden (tray
        // close, minimize), force-disarm the recognizer so it doesn't
        // keep capturing audio in the background. Overlays are separate
        // webviews and own their own arming/disarming, so we only wire
        // this on the main window. The cleanup is captured in a
        // component-scoped variable so onDestroy can reach it (async
        // onMount can't return a cleanup function in Svelte's typings).
        if (
            currentLabel !== 'overlay' &&
            currentLabel !== 'clipboard-overlay' &&
            currentLabel !== 'voice-overlay' &&
            currentLabel !== 'mouse-grid' &&
            currentLabel !== 'ui-elements' &&
            currentLabel !== 'welcome' &&
            !currentLabel.startsWith('quicknote')
        ) {
            const { installVisibilityWatcher } = await import('$lib/stores/voiceSession');
            voiceVisibilityCleanup = installVisibilityWatcher();
            // Global "is the mic hot" tracker — drives the StatusBar's
            // red-flickering "Listening" pill regardless of which
            // surface (overlay, page, dictation) is doing the capture.
            // Wired in the main window only since that's where the
            // StatusBar lives. Overlay webviews don't need it.
            const { initVoiceActivity } = await import('$lib/stores/voiceActivity');
            initVoiceActivity();
            // Push-to-talk: a persistent global handler that listens for
            // the backend's `voice-ptt-start` / `voice-ptt-stop` events
            // (emitted while the PTT hotkey is held) and orchestrates a
            // hold-to-dictate session. Main window only — overlay
            // webviews don't run this. No-op until the user enables PTT
            // in Settings.
            const { initPushToTalk } = await import('$lib/stores/pushToTalk');
            pushToTalkCleanup = initPushToTalk();
            // Command mode (Voice Commander Phase B): the coordinator
            // for the continuous, grammar-constrained voice-control
            // session. Main window only — it owns the session and the
            // command matcher. No-op until the user toggles command
            // mode on from the voice overlay.
            const { initCommandMode } = await import('$lib/stores/commandMode');
            commandModeCleanup = initCommandMode();
        }

        // Global screen-recording start/stop hotkey. Main window only — it owns
        // the recorder state, so the toggle works from ANY page (start when idle,
        // stop when recording). Stop while the app is minimized is handled by the
        // always-alive floating toolbar, which has its own listener.
        if (currentLabel === 'main') {
            const { toggleRecording } = await import('$lib/stores/screenRecorderControl');
            const { listen } = await import('@tauri-apps/api/event');
            screenRecToggleCleanup = await listen('screenrec:hotkey-toggle', () => {
                void toggleRecording();
            });
        }

        // #21 — user-authored voice commands. Loaded in every window
        // that matches voice commands (the main window, the search
        // overlay, the voice overlay) so each window's command registry
        // includes them. The backend file-watcher is started here too,
        // idempotently, so hot-reload works from whichever window runs.
        if (
            currentLabel === 'main' ||
            currentLabel === 'overlay' ||
            currentLabel === 'voice-overlay'
        ) {
            const { initUserVoiceCommands } = await import(
                '$lib/stores/userVoiceCommands'
            );
            userVoiceCommandsCleanup = initUserVoiceCommands();
            // Built-in command phrase overrides (the in-app editor): load +
            // subscribe so the registry + grammar use the user's customized
            // phrases in every command-matching window.
            const { initCommandOverrides } = await import(
                '$lib/stores/commandOverrides'
            );
            commandOverridesCleanup = initCommandOverrides();
        }

        // Window-level error handlers. These catch:
        //   - Errors thrown outside the render tree (async tasks, event
        //     listeners, promise rejections, dynamic imports, etc.)
        //   - Anything that would otherwise hit the browser's default
        //     "Uncaught error in module" white-screen.
        // We surface them through the same UI as render errors so users
        // get one consistent recovery path.
        window.addEventListener('error', (event) => {
            // Filter out resource-load errors (images failing to load,
            // etc.) — those have `event.error === null` and don't justify
            // tearing down the UI. Same for cross-origin script errors
            // (which we shouldn't have in a Tauri app anyway).
            if (event.error) {
                asyncError = event.error;
            }
        });
        window.addEventListener('unhandledrejection', (event) => {
            asyncError = event.reason ?? new Error('Unhandled promise rejection');
        });

        // ALSO route both to the persistent diagnostic log so the user
        // can attach the trail to a bug report later. The error UI
        // above is for in-the-moment recovery; the log is for forensic
        // review. Lives in a separate module so the toast pipeline
        // can also write to the same log.
        const { installGlobalErrorCapture } = await import('$lib/stores/errorLog');
        errorCaptureCleanup = installGlobalErrorCapture();

        // Poll backend init health on the main window only. The backend
        // collects hotkey + clipboard failures during setup into a list;
        // we drain it here and surface each one as a dismissable banner.
        // Similarly, if the preferences DB was auto-recovered from a
        // corruption event, we show a one-time data-loss warning.
        if (currentLabel === 'main') {
            try {
                const issues = await invoke<string[]>('get_backend_init_issues');
                if (issues.length > 0) {
                    initWarnings = issues;
                }
            } catch {
                // Non-critical — silently ignore if the command fails.
            }
            try {
                const wasCorrupted = await invoke<boolean>('take_db_corruption_notice');
                if (wasCorrupted) {
                    dbCorruptionRecovered = true;
                }
            } catch {
                // Non-critical — silently ignore.
            }
        }

        try {
            await initSettingsStore();
            await initToolPacksStore();
            // Quick-note stays RAM-light: it needs the theme (applied below)
            // but none of the profile / shredder / archive machinery.
            if (currentLabel !== 'overlay' && !currentLabel.startsWith('quicknote')) {
                const { initProfilesStore } = await import('$lib/stores/profiles');
                await initProfilesStore();
                const enabled = new Set(get(enabledPackIds));
                if (enabled.has('privacy')) {
                    const { initShredderListeners } = await import('$lib/stores/shredder');
                    await initShredderListeners();
                }
            }
            document.documentElement.dataset.theme = $settings.theme;
            // Apply the persisted UI font on first paint (mirrors theme) so
            // there's no flash of Inter before the chosen font kicks in.
            // settings.subscribe keeps it in sync on later changes.
            applyFont($settings.uiFont);
            // Opt-in automatic Privacy Audit. Main window only (it owns the
            // single long-lived scheduler timer); the module also self-gates
            // to the main window. No-op until the user picks a cadence in
            // Settings → System — fully local, notifies only on findings.
            if (currentLabel === 'main') {
                const { initPrivacyAuditScheduler, stopPrivacyAuditScheduler } = await import(
                    '$lib/stores/privacyAuditScheduler'
                );
                await initPrivacyAuditScheduler();
                privacyAuditSchedulerCleanup = stopPrivacyAuditScheduler;
            }
        } catch (error) {
            // Init-time failures fall into the same recovery UI rather
            // than leaving the user with a half-bootstrapped app.
            asyncError = error;
        }
    });

    onDestroy(() => {
        voiceVisibilityCleanup?.();
        voiceVisibilityCleanup = undefined;
        pushToTalkCleanup?.();
        pushToTalkCleanup = undefined;
        commandModeCleanup?.();
        commandModeCleanup = undefined;
        errorCaptureCleanup?.();
        errorCaptureCleanup = undefined;
        userVoiceCommandsCleanup?.();
        userVoiceCommandsCleanup = undefined;
        commandOverridesCleanup?.();
        commandOverridesCleanup = undefined;
        privacyAuditSchedulerCleanup?.();
        privacyAuditSchedulerCleanup = undefined;
        screenRecToggleCleanup?.();
        screenRecToggleCleanup = undefined;
    });

    function resetAsync() {
        asyncError = null;
    }
</script>

<!--
  Layered error guard:
    1. <svelte:boundary> catches render-time errors thrown inside any
       child component tree. It captures the error + a reset callback
       and lets us substitute the GlobalErrorScreen UI without losing
       the rest of the app shell.
    2. The `asyncError` overlay handles non-render errors caught by
       the window-level listeners in onMount. We render this OUTSIDE
       the boundary so an async error in the GlobalErrorScreen itself
       (extremely unlikely, but defensively…) doesn't loop forever.
-->
<!--
  Backend init warning banners — dismissable per-entry. Shown above
  the app content only when startup had non-fatal issues (e.g. a hotkey
  couldn't be registered because another app holds it). Each banner is
  independent so the user can dismiss just the ones they've read.
-->
{#if dbCorruptionRecovered}
    <div class="init-warning-banner" role="alert">
        <span class="init-warning-icon">⚠</span>
        <span class="init-warning-text">
            Your preferences database was corrupted and has been reset. A backup of the
            corrupt file was saved to your app data folder. Your tool outputs are unaffected.
        </span>
        <button
            type="button"
            class="init-warning-dismiss"
            onclick={() => (dbCorruptionRecovered = false)}
            aria-label="Dismiss"
        >✕</button>
    </div>
{/if}
{#each initWarnings as warning, i (warning)}
    <div class="init-warning-banner" role="alert">
        <span class="init-warning-icon">⚠</span>
        <span class="init-warning-text">{warning}</span>
        <button
            type="button"
            class="init-warning-dismiss"
            onclick={() => (initWarnings = initWarnings.filter((_, idx) => idx !== i))}
            aria-label="Dismiss"
        >✕</button>
    </div>
{/each}

<svelte:boundary>
    {#snippet failed(error, reset)}
        <GlobalErrorScreen {error} {reset} />
    {/snippet}

    {@render children()}
</svelte:boundary>

{#if asyncError}
    <GlobalErrorScreen error={asyncError} reset={resetAsync} />
{/if}

<!--
  Themed confirm dialog — singleton driven by the `activeConfirm`
  store. Mounted in every window (main + search overlay + clipboard
  overlay + voice overlay) since the layout wraps every route. Each
  window's instance manages its own dialog queue independently.

  Imported from $lib/components/ConfirmDialog.svelte; the `confirm()`
  helper in $lib/stores/confirmDialog mirrors the Tauri plugin's API.
-->
<ConfirmDialog />

<style>
    .init-warning-banner {
        position: fixed;
        top: 0;
        left: 0;
        right: 0;
        z-index: 9999;
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.5rem 0.75rem;
        background: color-mix(in srgb, var(--color-warning, #f59e0b) 18%, var(--color-panel-1, #1a1a1a));
        border-bottom: 1px solid color-mix(in srgb, var(--color-warning, #f59e0b) 40%, transparent);
        font-size: 0.8125rem;
        line-height: 1.4;
        color: var(--color-text, #e5e5e5);
    }
    .init-warning-icon {
        flex-shrink: 0;
        color: var(--color-warning, #f59e0b);
    }
    .init-warning-text {
        flex: 1;
    }
    .init-warning-dismiss {
        flex-shrink: 0;
        background: none;
        border: none;
        cursor: pointer;
        color: var(--color-text-muted, #a1a1aa);
        font-size: 0.875rem;
        padding: 0.125rem 0.25rem;
        border-radius: 4px;
        transition: color 120ms;
    }
    .init-warning-dismiss:hover {
        color: var(--color-text, #e5e5e5);
    }
</style>

