<script lang="ts">
    /*
      Voice to Text — Phase 3.2 redesign.

      The page is a *recording surface*, not a stats page. The mic is
      the focus; results and transcript come and go around it.

      Backend / state machine preserved EXACTLY:
        - `voice_check_availability` on mount → engineCompiled / engineCompiledHint.
        - `voice_recognize_once` (single-shot) + `voice_cancel_recognize`.
        - `voice_start_continuous` / `voice_stop_continuous` for dictation sessions.
        - `voice_release_models` idle countdown (60s) after a single-shot.
        - `voice-partial` / `voice-final` / `voice-preempted` event subscriptions,
          all filtered by `source === 'voice-to-text-page'`.
        - `execute_system_command` → `microphone-privacy` on permission denial.
        - `reportBusy` for global busy state.
        - `recordActivity` for activity log.
        - `processDictation` + `normalizeTranscript` for in-dictation formatting.
        - `availability` derivation (engineCompiled + voskModelPath).
        - `confidenceLabel` derivation for the result chip.

      Surface changes:
        - Hero + radial-gradient block DROPPED. Replaced with the calm
          ToolPage header (Mic icon + title + tagline).
        - Probing state → ToolPanel with a LoadingState primitive.
        - Engine-not-compiled / model-needed states → ErrorState +
          ToolPanel CTA blocks (consistent with the rest of the kit).
        - Mic surface restyled as a centered ToolPanel: bigger mic
          (104px), inset top-lit highlight to match the macOS-style
          ToolPage icon tile, status caption uses the next-up text size.
        - Continuous toggle moved to kit Button (variants: secondary
          when offering "start", danger when offering "stop").
        - Latest / Transcript cards become ToolPanels with consistent
          header + actions row + content rhythm.
        - Tips line moved into the ToolPage footer slot (keyboard hints
          + tips share that strip).
    */
    import { onMount, onDestroy } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { listen, type UnlistenFn } from '@tauri-apps/api/event';
    import { confirm } from '$lib/stores/confirmDialog';
    import {
        Mic,
        Square,
        Copy,
        ShieldCheck,
        AlertTriangle,
        Trash2,
        Settings as SettingsIcon,
        Plus,
    } from '@lucide/svelte';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import { recordActivity } from '$lib/stores/activityLog';
    import { reportBusy } from '$lib/stores/globalBusy';
    import { settings } from '$lib/stores/settings';
    import { processDictation } from '$lib/stores/dictationCommands';
    import { normalizeTranscript } from '$lib/stores/transcriptNormalizer';
    import { voiceModelLocale } from '$lib/stores/commandRegistry';
    // Sprint 3 — Tool Quality Pass (2026-05-27): i18n imported so the
    // page's 5 previously-hardcoded English strings (title, two button
    // labels, the mic-access guidance) flow through the translation
    // pipeline. Feeds Phase 7 (Georgian localization).
    import { _ } from 'svelte-i18n';
    // Deep-link channel for the Settings page. Setting this BEFORE
    // flipping `selected = 'settings'` makes Settings.svelte mount with
    // the Voice section pre-selected — the user lands exactly where the
    // model needs to be picked.
    import { requestSettingsSection } from '$lib/stores/settingsTarget';
    // Phase 1 kit primitives. No hand-rolled boxes.
    import {
        ToolPage,
        ToolPanel,
        Button,
        LoadingState,
        ErrorState,
    } from '$lib/ui';

    /** Two-way bound nav prop — set to navigate the main window to a
     *  different sidebar destination. We use it to send the user
     *  straight to Settings → Voice when they hit a setup card. */
    let { selected = $bindable('voice-to-text') }: { selected?: string } = $props();

    type Availability = {
        available: boolean;
        hint: string | null;
    };

    type RecognitionResult = {
        text: string;
        confidence: string;
        status: string;
    };

    let engineCompiled = $state<boolean | null>(null);
    let engineCompiledHint = $state<string | null>(null);

    /** UI availability — accounts for the build fact + the user's
     *  model-folder choice. Drives the setup-vs-mic UI flip. */
    let availability = $derived.by<Availability | null>(() => {
        if (engineCompiled === null) return null;
        if (!engineCompiled) {
            return {
                available: false,
                hint: engineCompiledHint ?? "Voice support isn't compiled into this build.",
            };
        }
        const path = $settings.voskModelPath?.trim() ?? '';
        if (path.length === 0) {
            return {
                available: false,
                hint:
                    'Voice recognition needs a language pack. ' +
                    'Open Settings → Voice and install one (40 MB starter pack for English).',
            };
        }
        return { available: true, hint: null };
    });

    let listening = $state(false);
    let continuousMode = $state(false);
    let stopRequested = $state(false);

    let latestText = $state('');
    let latestConfidence = $state<string | null>(null);
    let latestStatus = $state<string | null>(null);

    let transcript = $state('');

    let partialText = $state<string | null>(null);
    let unlistenPartial: UnlistenFn | null = null;
    let unlistenFinal: UnlistenFn | null = null;
    let unlistenPreempted: UnlistenFn | null = null;

    function applyPartialEvent(text: string) {
        partialText = text;
    }
    function clearPartialState() {
        partialText = null;
    }

    /** True whenever ANY recognition path is active. */
    let micActive = $derived(listening || continuousMode);

    async function checkEngineCompiled() {
        try {
            const result = await invoke<Availability>('voice_check_availability');
            engineCompiled = result.available;
            engineCompiledHint = result.hint;
        } catch (error) {
            engineCompiled = false;
            engineCompiledHint = `Could not check voice support: ${error}`;
        }
    }

    onMount(async () => {
        await checkEngineCompiled();

        unlistenPartial = await listen<{ text: string; source: string | null }>(
            'voice-partial',
            (evt) => {
                const evtSource = evt.payload?.source;
                if (evtSource && evtSource !== 'voice-to-text-page') return;
                applyPartialEvent(evt.payload?.text ?? '');
            },
        );

        unlistenPreempted = await listen<{ client?: string }>(
            'voice-preempted',
            (evt) => {
                if (evt.payload?.client !== 'voice-to-text-page') return;
                if (continuousMode) {
                    stopRequested = true;
                    unlistenFinal?.();
                    unlistenFinal = null;
                    continuousMode = false;
                    clearPartialState();
                    toast(
                        'Microphone taken by another voice surface — dictation stopped.',
                        'info',
                        4000,
                    );
                }
            },
        );
    });

    onDestroy(() => {
        unlistenPartial?.();
        unlistenPartial = null;
        unlistenFinal?.();
        unlistenFinal = null;
        unlistenPreempted?.();
        unlistenPreempted = null;
        cancelModelRelease();
        void invoke('voice_stop_continuous').catch(() => {});
        void invoke('voice_cancel_recognize').catch(() => {});
    });

    /* ─── Idle model release (preserved verbatim) ──────────────── */
    const MODEL_IDLE_RELEASE_MS = 60_000;
    let modelReleaseTimer: ReturnType<typeof setTimeout> | null = null;

    function cancelModelRelease() {
        if (modelReleaseTimer) {
            clearTimeout(modelReleaseTimer);
            modelReleaseTimer = null;
        }
    }
    function scheduleModelRelease() {
        cancelModelRelease();
        modelReleaseTimer = setTimeout(() => {
            modelReleaseTimer = null;
            void invoke('voice_release_models').catch(() => {});
        }, MODEL_IDLE_RELEASE_MS);
    }

    async function recognizeOnce(): Promise<RecognitionResult | null> {
        const stopBusy = reportBusy('voice-to-text', 'Listening…');
        listening = true;
        cancelModelRelease();
        clearPartialState();
        try {
            const result = await invoke<RecognitionResult>('voice_recognize_once', {
                modelPath: $settings.voskModelPath || null,
                source: 'voice-to-text-page',
            });
            return result;
        } catch (error) {
            toast(`Voice recognition failed: ${error}`, 'error');
            return null;
        } finally {
            listening = false;
            clearPartialState();
            stopBusy();
            scheduleModelRelease();
        }
    }

    async function singleShot() {
        if (listening) {
            void invoke('voice_cancel_recognize').catch(() => {});
            return;
        }
        const result = await recognizeOnce();
        if (!result) return;
        applyResult(result);
    }

    async function startContinuous() {
        if (listening || continuousMode) return;
        continuousMode = true;
        stopRequested = false;
        clearPartialState();
        cancelModelRelease();
        toast('Continuous dictation started — click Stop when finished.', 'info', 3500);
        await startContinuousBackendSession();
    }

    async function startContinuousBackendSession() {
        unlistenFinal = await listen<{ text: string; source: string | null }>(
            'voice-final',
            (evt) => {
                if (evt.payload?.source !== 'voice-to-text-page') return;
                const text = (evt.payload?.text ?? '').trim();
                if (text.length === 0) return;
                const formatted = processDictation(
                    normalizeTranscript(text, { mode: 'dictation' }).normalized,
                    voiceModelLocale(),
                );
                if (formatted.length > 0) {
                    transcript = transcript ? `${transcript} ${formatted}` : formatted;
                }
                latestText = text;
                latestStatus = 'success';
                latestConfidence = 'high';
                clearPartialState();
                void recordActivity({
                    toolId: 'voice-to-text',
                    summary: `Transcribed ${text.length} char${text.length === 1 ? '' : 's'}`,
                    outcome: 'success',
                });
            },
        );

        try {
            await invoke('voice_start_continuous', {
                modelPath: $settings.voskModelPath || null,
                source: 'voice-to-text-page',
            });
        } catch (error) {
            const msg = String(error);
            if (msg.includes('vosk_setup_required')) {
                toast(
                    'Voice recognition needs a language pack — install one in Settings → Voice.',
                    'error',
                    7000,
                );
            } else if (msg.includes('vosk_not_compiled')) {
                toast("Voice recognition isn't available in this build.", 'error', 8000);
            } else if (msg.toLowerCase().includes('access is denied')) {
                void invoke('execute_system_command', { id: 'microphone-privacy' }).catch(
                    () => {},
                );
                toast(
                    'Microphone access denied. Allow desktop apps in Settings → Privacy → Microphone.',
                    'error',
                    9000,
                );
            } else if (msg.includes('mic_busy')) {
                toast('Microphone is in use by another voice surface.', 'info', 4000);
            } else {
                errorToast("Couldn't start continuous dictation", error, {
                    hint: 'Try toggling the mic off and on, or restart KeepItLocal. If the problem persists, check that another app isn\'t using the microphone.',
                    durationMs: 6500,
                });
            }
            unlistenFinal?.();
            unlistenFinal = null;
            continuousMode = false;
        }
    }

    async function stopContinuous() {
        stopRequested = true;
        cancelModelRelease();
        await Promise.all([
            invoke('voice_stop_continuous').catch(() => {}),
            invoke('voice_cancel_recognize').catch(() => {}),
        ]);
        unlistenFinal?.();
        unlistenFinal = null;
        continuousMode = false;
        clearPartialState();
    }

    function applyResult(result: RecognitionResult, appendToTranscript = false) {
        latestText = result.text;
        latestConfidence = result.confidence;
        latestStatus = result.status;

        if (result.status === 'success' && result.text.trim().length > 0) {
            if (appendToTranscript) {
                const formatted = processDictation(
                    normalizeTranscript(result.text, { mode: 'dictation' }).normalized,
                    voiceModelLocale(),
                );
                if (formatted.length > 0) {
                    transcript = transcript ? `${transcript} ${formatted}` : formatted;
                }
            }
            void recordActivity({
                toolId: 'voice-to-text',
                summary: `Transcribed ${result.text.length} char${result.text.length === 1 ? '' : 's'}`,
                outcome: 'success',
            });
        } else if (result.status === 'no_speech') {
            // Silence — no toast.
        } else if (result.status === 'mic_busy') {
            toast('Microphone is in use by another voice surface.', 'info', 4000);
            clearLatest();
        } else if (result.status === 'permission_denied') {
            void invoke('execute_system_command', { id: 'microphone-privacy' }).catch(() => {});
            toast(
                $_('tool.voiceToText.micAccessDenied'),
                'error',
                9000,
            );
            stopContinuous();
        } else if (result.status === 'audio_error') {
            toast('Audio quality too low. Check your microphone.', 'error');
        } else if (result.status === 'vosk_setup_required') {
            toast(
                'Vosk needs a model. Open Settings → Voice and pick a model folder.',
                'error',
                7000,
            );
            stopContinuous();
        } else if (result.status === 'vosk_not_compiled') {
            toast("Voice recognition isn't available in this build.", 'error', 7000);
            stopContinuous();
        }
    }

    async function copyText(value: string) {
        if (!value) return;
        try {
            await navigator.clipboard.writeText(value);
            toast('Copied to clipboard', 'success');
        } catch (error) {
            toast(`Copy failed: ${error}`, 'error');
        }
    }

    function appendLatestToTranscript() {
        if (!latestText.trim()) return;
        const formatted = processDictation(
            normalizeTranscript(latestText, { mode: 'dictation' }).normalized,
            voiceModelLocale(),
        );
        if (formatted.length === 0) return;
        transcript = transcript ? `${transcript} ${formatted}` : formatted;
        toast('Added to transcript', 'success');
    }

    async function clearTranscript() {
        if (!transcript) return;
        const ok = await confirm('Clear the full transcript?', {
            title: $_('tool.voiceToText.clearTranscript'),
            kind: 'warning',
        });
        if (ok) {
            transcript = '';
        }
    }

    function clearLatest() {
        latestText = '';
        latestConfidence = null;
        latestStatus = null;
    }

    /** Friendly label for the result-confidence chip. */
    let confidenceLabel = $derived.by(() => {
        switch (latestConfidence) {
            case 'high':
                return { label: 'High confidence', tone: 'success' };
            case 'medium':
                return { label: 'Medium confidence', tone: 'info' };
            case 'low':
                return { label: 'Low confidence', tone: 'warning' };
            case 'rejected':
                return { label: 'Rejected', tone: 'error' };
            default:
                return null;
        }
    });

    /** Caption shown under the mic — mirrors the old surface's logic. */
    let micCaption = $derived.by(() => {
        if (partialText) return { text: `${partialText}…`, italic: true };
        if (continuousMode)
            return { text: 'Continuous dictation running — speak any time', italic: false };
        if (listening) return { text: 'Listening…', italic: false };
        return { text: 'Click the mic to record one phrase', italic: false };
    });
</script>

<ToolPage
    icon={Mic}
    title={$_('tool.voiceToText.title')}
    description="Speech recognition runs entirely on this PC. Your microphone audio is processed locally — no cloud, no account, no internet."
    width="medium"
>
    {#if availability === null}
        <ToolPanel padding="lg">
            <LoadingState label="Checking voice recognition availability…" />
        </ToolPanel>
    {:else if !availability.available}
        {#if !engineCompiled}
            <!-- Build doesn't ship a voice engine — nothing the user
                 can self-service. Surface the backend hint via the
                 ErrorState primitive (no retry — there's nothing to
                 retry against). -->
            <ErrorState
                title="Voice support isn't available"
                description={availability.hint ??
                    "Voice support isn't compiled into this build."}
            />
        {:else}
            <!-- Engine compiled, model not configured. Setup card with
                 a clear path to Settings → Voice. -->
            <ToolPanel padding="lg">
                <div class="setup">
                    <div class="setup-icon" aria-hidden="true">
                        <AlertTriangle class="setup-icon-svg" />
                    </div>
                    <div class="setup-body">
                        <h2 class="setup-title">Voice needs a language pack</h2>
                        <p class="setup-desc">
                            Install a small language pack to enable on-device voice recognition.
                            The 40&nbsp;MB starter pack for English is a good first pick — it stays
                            on this machine and works fully offline.
                        </p>
                        <div class="setup-steps">
                            <div class="setup-steps-label">Quick fix</div>
                            <ol class="setup-steps-list">
                                <li>
                                    Open <strong>Settings → Voice</strong> and use the one-click
                                    install — we download and unpack the language pack for you.
                                </li>
                                <li>
                                    Already downloaded one yourself? Settings → Voice → Advanced
                                    lets you point at an existing folder. (For the curious: it's a
                                    Vosk model — open <a
                                        href="https://alphacephei.com/vosk/models"
                                        target="_blank"
                                        rel="noopener noreferrer"
                                        class="setup-link"
                                    >alphacephei.com/vosk/models</a>.)
                                </li>
                                <li>Come back here — the mic appears as soon as the language pack is installed.</li>
                            </ol>
                        </div>
                        <div class="setup-actions">
                            <Button
                                variant="primary"
                                icon={SettingsIcon}
                                onclick={() => {
                                    // Pre-select the Voice section so the user
                                    // lands directly on the model picker, not
                                    // on Settings' default section.
                                    requestSettingsSection('voice');
                                    selected = 'settings';
                                }}
                            >
                                Open Settings → Voice
                            </Button>
                        </div>
                    </div>
                </div>
            </ToolPanel>
        {/if}
    {:else}
        <!-- Mic surface — the page's focus. Centered, breathy, with
             the recording affordance front and center. -->
        <ToolPanel padding="lg">
            <div class="mic-surface">
                <button
                    type="button"
                    onclick={singleShot}
                    disabled={continuousMode}
                    class="mic-button"
                    class:is-live={micActive}
                    aria-label={continuousMode
                        ? 'Continuous dictation running'
                        : listening
                          ? 'Listening — click to cancel'
                          : 'Click to record one phrase'}
                    title={continuousMode
                        ? 'Continuous dictation running — use Stop button below'
                        : listening
                          ? 'Click to cancel recording'
                          : 'Click to record one phrase'}
                >
                    {#if micActive}
                        <span class="mic-pulse" aria-hidden="true"></span>
                    {/if}
                    <Mic class="mic-icon" />
                </button>

                <p class="mic-caption" class:is-italic={micCaption.italic}>
                    {micCaption.text}
                </p>

                <div class="mic-actions">
                    {#if !continuousMode}
                        <Button
                            variant="secondary"
                            size="sm"
                            icon={Mic}
                            onclick={startContinuous}
                            disabled={listening}
                        >
                            Continuous dictation
                        </Button>
                    {:else}
                        <Button
                            variant="danger"
                            size="sm"
                            icon={Square}
                            onclick={() => void stopContinuous()}
                        >
                            Stop dictation
                        </Button>
                    {/if}
                </div>

                <!-- Privacy reassurance — quiet, accent-tinted, lives
                     in the mic surface so it reads as "what the mic
                     does" rather than a separate marketing block. -->
                <div class="mic-privacy">
                    <ShieldCheck class="mic-privacy-ico" />
                    <span>All recognition happens on this PC.</span>
                </div>
            </div>
        </ToolPanel>

        <!-- Latest utterance — only appears when there's something to show. -->
        {#if latestText || latestStatus}
            <ToolPanel padding="md">
                <header class="vt-block-head">
                    <div class="vt-block-titles">
                        <span class="vt-block-label">Latest</span>
                        {#if confidenceLabel}
                            <span class="vt-conf vt-conf-{confidenceLabel.tone}">
                                {confidenceLabel.label}
                            </span>
                        {/if}
                        {#if latestStatus === 'no_speech'}
                            <span class="vt-no-speech">Nothing heard.</span>
                        {/if}
                    </div>
                    {#if latestText}
                        <div class="vt-block-actions">
                            <Button
                                size="sm"
                                variant="ghost"
                                icon={Copy}
                                onclick={() => void copyText(latestText)}
                            >
                                Copy
                            </Button>
                            <Button
                                size="sm"
                                variant="ghost"
                                icon={Plus}
                                onclick={appendLatestToTranscript}
                            >
                                Add to transcript
                            </Button>
                            <Button
                                size="sm"
                                variant="ghost"
                                iconOnly
                                icon={Trash2}
                                title={$_('tool.voiceToText.clearLatest')}
                                aria-label={$_('tool.voiceToText.clearLatest')}
                                onclick={clearLatest}
                            />
                        </div>
                    {/if}
                </header>

                <div class="vt-result">
                    {#if latestText}
                        <p class="vt-result-text">{latestText}</p>
                    {:else}
                        <p class="vt-result-empty">
                            Speak and the recognized text appears here.
                        </p>
                    {/if}
                </div>
            </ToolPanel>
        {/if}

        <!-- Running transcript — accumulates across utterances. -->
        <ToolPanel padding="md">
            <header class="vt-block-head">
                <div class="vt-block-titles">
                    <span class="vt-block-label">Transcript</span>
                    <span class="vt-block-count">
                        {transcript.length} character{transcript.length === 1 ? '' : 's'}
                    </span>
                </div>
                {#if transcript}
                    <div class="vt-block-actions">
                        <Button
                            size="sm"
                            variant="ghost"
                            icon={Copy}
                            onclick={() => void copyText(transcript)}
                        >
                            Copy all
                        </Button>
                        <Button
                            size="sm"
                            variant="ghost"
                            iconOnly
                            icon={Trash2}
                            title={$_('tool.voiceToText.clearTranscript')}
                            aria-label={$_('tool.voiceToText.clearTranscript')}
                            onclick={() => void clearTranscript()}
                        />
                    </div>
                {/if}
            </header>

            {#if transcript}
                <textarea
                    bind:value={transcript}
                    class="vt-transcript"
                    spellcheck="true"
                ></textarea>
                <p class="vt-transcript-hint">
                    Edit freely — recognition isn't perfect. Use Continuous dictation
                    above to keep adding utterances.
                </p>
            {:else}
                <div class="vt-transcript-empty">
                    Start <strong>Continuous dictation</strong> above, or use "Add to
                    transcript" on each phrase, to build up a longer text here.
                </div>
            {/if}
        </ToolPanel>
    {/if}

    {#snippet footer()}
        <span class="vt-tip">
            <strong>Tips:</strong>
            speak clearly at a normal pace · pause briefly to end an utterance · edit
            the transcript afterwards to fix any punctuation or wording.
        </span>
    {/snippet}
</ToolPage>

<style>
    /* ─── Setup card (engine compiled, model missing) ──────────────
       Same content as the old surface, restyled to compose from
       ToolPanel + Button. */
    .setup {
        display: flex;
        align-items: flex-start;
        gap: 14px;
    }
    .setup-icon {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 40px;
        height: 40px;
        border-radius: 10px;
        background: var(--color-warning-soft);
        color: var(--color-warning);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 8%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 4%, transparent);
    }
    .setup :global(.setup-icon-svg) {
        width: 20px;
        height: 20px;
    }
    .setup-body {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .setup-title {
        margin: 0;
        font-size: 16px;
        font-weight: 600;
        letter-spacing: -0.008em;
        color: var(--color-text);
    }
    .setup-desc {
        margin: 0;
        font-size: 13px;
        line-height: 1.55;
        color: var(--color-text-secondary);
        max-width: 64ch;
    }
    .setup-link {
        color: var(--color-accent);
        text-decoration: underline;
        text-decoration-color: color-mix(in srgb, var(--color-accent) 50%, transparent);
        text-underline-offset: 2px;
    }
    .setup-link:hover {
        text-decoration-color: var(--color-accent);
    }
    .setup-code {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        padding: 1px 6px;
        border-radius: 4px;
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .setup-code-tiny {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 10.5px;
        padding: 1px 5px;
        border-radius: 4px;
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .setup-steps {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }
    .setup-steps-label {
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }
    .setup-steps-list {
        margin: 0;
        padding-left: 20px;
        font-size: 12.5px;
        line-height: 1.6;
        color: var(--color-text-secondary);
        display: flex;
        flex-direction: column;
        gap: 4px;
    }
    .setup-steps-list strong {
        color: var(--color-text);
        font-weight: 600;
    }
    .setup-actions {
        margin-top: 4px;
    }

    /* ─── Mic surface ──────────────────────────────────────────────
       Centered, breathy, the page's focus. The mic is large enough
       to feel like an affordance, with an inset top-lit highlight
       to match the macOS-style tile look used in ToolPage's icon. */
    .mic-surface {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 18px;
        padding: 20px 0 16px;
    }

    .mic-button {
        position: relative;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 104px;
        height: 104px;
        border-radius: 50%;
        border: 2px solid color-mix(in srgb, var(--color-accent) 65%, var(--color-border));
        background: color-mix(in srgb, var(--color-accent) 12%, transparent);
        color: var(--color-accent);
        cursor: pointer;
        /* macOS-style lit-from-above feel — same inset recipe as
           ToolPage's icon tile and the overlay panel. */
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 12%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 4%, transparent),
            0 4px 14px color-mix(in srgb, var(--color-accent) 18%, transparent);
        transition:
            transform 140ms var(--ease-out),
            background-color 140ms var(--ease-out),
            box-shadow 180ms var(--ease-out),
            border-color 140ms var(--ease-out),
            color 140ms var(--ease-out);
    }
    .mic-button:hover:not(:disabled) {
        transform: scale(1.04);
        background: color-mix(in srgb, var(--color-accent) 22%, transparent);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 12%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 4%, transparent),
            0 10px 28px color-mix(in srgb, var(--color-accent) 30%, transparent);
    }
    .mic-button:active:not(:disabled) {
        transform: scale(0.98);
    }
    .mic-button:disabled:not(.is-live) {
        opacity: 0.5;
        cursor: not-allowed;
    }
    .mic-button.is-live {
        background: var(--color-error);
        border-color: var(--color-error);
        color: var(--color-bg);
        cursor: pointer;
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 18%, transparent),
            0 6px 18px color-mix(in srgb, var(--color-error) 40%, transparent);
    }
    :global(.mic-icon) {
        width: 38px;
        height: 38px;
        position: relative;
        z-index: 2;
    }

    /* Pulsing ring during recording — error-toned to mirror the
       universal "live capture" language. */
    .mic-pulse {
        position: absolute;
        inset: -8px;
        border-radius: 50%;
        border: 3px solid var(--color-error);
        opacity: 0.6;
        animation: mic-pulse 1.4s ease-out infinite;
        pointer-events: none;
    }
    @keyframes mic-pulse {
        0% {
            transform: scale(0.85);
            opacity: 0.7;
        }
        100% {
            transform: scale(1.4);
            opacity: 0;
        }
    }

    .mic-caption {
        margin: 0;
        font-size: 14px;
        line-height: 1.45;
        font-weight: 500;
        color: var(--color-text);
        text-align: center;
        max-width: 48ch;
        letter-spacing: -0.005em;
    }
    .mic-caption.is-italic {
        font-style: italic;
        color: var(--color-text-secondary);
        font-weight: 400;
    }

    .mic-actions {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-wrap: wrap;
        justify-content: center;
    }

    .mic-privacy {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        margin-top: 4px;
        padding: 5px 11px;
        border-radius: var(--radius-pill);
        background: var(--color-accent-soft);
        border: 1px solid color-mix(in srgb, var(--color-accent) 26%, var(--color-border));
        color: var(--color-accent);
        font-size: 11.5px;
        font-weight: 500;
    }
    .mic-surface :global(.mic-privacy-ico) {
        width: 13px;
        height: 13px;
    }

    /* ─── Latest / Transcript blocks ───────────────────────────── */
    .vt-block-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        margin-bottom: 10px;
        flex-wrap: wrap;
    }
    .vt-block-titles {
        display: flex;
        align-items: center;
        gap: 10px;
        flex-wrap: wrap;
        min-width: 0;
    }
    .vt-block-label {
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }
    .vt-block-count {
        font-size: 11.5px;
        color: var(--color-muted);
        font-variant-numeric: tabular-nums;
    }
    .vt-block-actions {
        display: flex;
        align-items: center;
        gap: 4px;
        flex-wrap: wrap;
    }

    /* Confidence chip — color-coded, quiet pill. */
    .vt-conf {
        display: inline-flex;
        align-items: center;
        height: 20px;
        padding: 0 9px;
        border-radius: var(--radius-pill);
        font-size: 11px;
        font-weight: 600;
        letter-spacing: -0.002em;
    }
    .vt-conf-success {
        background: var(--color-success-soft);
        color: var(--color-success);
        border: 1px solid var(--color-success-strong);
    }
    .vt-conf-info {
        background: var(--color-info-soft);
        color: var(--color-info);
        border: 1px solid var(--color-info-strong);
    }
    .vt-conf-warning {
        background: var(--color-warning-soft);
        color: var(--color-warning);
        border: 1px solid var(--color-warning-strong);
    }
    .vt-conf-error {
        background: var(--color-error-soft);
        color: var(--color-error);
        border: 1px solid var(--color-error-strong);
    }
    .vt-no-speech {
        font-size: 11.5px;
        color: var(--color-muted);
        font-style: italic;
    }

    .vt-result {
        background: var(--color-bg);
        border: 1px solid var(--color-divider, var(--color-border));
        border-radius: var(--radius-control);
        padding: 12px 14px;
        min-height: 60px;
    }
    .vt-result-text {
        margin: 0;
        font-size: 15px;
        line-height: 1.55;
        color: var(--color-text);
        letter-spacing: -0.005em;
    }
    .vt-result-empty {
        margin: 0;
        font-size: 12px;
        font-style: italic;
        color: var(--color-muted);
    }

    .vt-transcript {
        width: 100%;
        min-height: 180px;
        max-height: 40vh;
        padding: 12px 14px;
        background: var(--color-bg);
        border: 1px solid var(--color-divider, var(--color-border));
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-size: 14px;
        line-height: 1.55;
        resize: vertical;
        outline: none;
        transition: border-color var(--dur-micro) var(--ease-out);
    }
    .vt-transcript:focus {
        border-color: var(--color-accent);
    }
    .vt-transcript-hint {
        margin: 6px 2px 0;
        font-size: 11px;
        color: var(--color-muted);
    }
    .vt-transcript-empty {
        padding: 18px 14px;
        background: var(--color-panel-2);
        border: 1px dashed var(--color-divider, var(--color-border));
        border-radius: var(--radius-control);
        color: var(--color-text-secondary);
        font-size: 12.5px;
        line-height: 1.55;
        text-align: center;
    }
    .vt-transcript-empty strong {
        color: var(--color-text);
        font-weight: 600;
    }

    /* ─── Footer tip strip ────────────────────────────────────── */
    .vt-tip {
        font-size: 11.5px;
        line-height: 1.55;
        color: var(--color-muted);
    }
    .vt-tip strong {
        color: var(--color-text-secondary);
        font-weight: 600;
    }
</style>
