<script lang="ts">
    /*
      VoiceCommandFeedback — the main window's voice-command feedback +
      confirmation surface.

      Voice upgrade, Layer 5 (#16). Two jobs, one compact floating card
      above the status bar:

        1. Confirmation prompt. When the #14 safety gate parks a
           dangerous command it sets `pendingConfirmation`. This card
           renders it — the command, a countdown to the auto-expiry, and
           Confirm / Cancel buttons. The buttons are the click fallback;
           the primary path is still spoken ("confirm" / "cancel"). The
           prompt takes priority over feedback — it is a decision the
           user owes an answer to.

        2. Command feedback. Otherwise it shows the most recent
           `voiceFeedback` line — what was heard and what happened — so
           command mode, which has no surface of its own, is visibly
           responsive.

      Mounted once in the main window (where command mode runs). Renders
      nothing when there is neither a pending confirmation nor recent
      feedback, so it is visually free while idle. The feedback variant
      is `pointer-events: none` — purely informational, it never blocks
      the UI beneath it; the confirmation variant is interactive.
    */
    import { Mic, AlertTriangle, Check, X } from '@lucide/svelte';
    import {
        pendingConfirmation,
        confirmPending,
        cancelPending,
        CONFIRMATION_EXPIRY_MS,
    } from '$lib/stores/voiceSafetyGate';
    import { voiceFeedback } from '$lib/stores/voiceFeedback';

    /** Re-read on a timer while a confirmation is pending so the
     *  countdown bar + seconds tick down. Idle otherwise — the effect
     *  only installs the interval when something is parked. */
    let now = $state(Date.now());

    $effect(() => {
        if (!$pendingConfirmation) return;
        now = Date.now();
        const id = setInterval(() => {
            now = Date.now();
        }, 150);
        return () => clearInterval(id);
    });

    let remainingMs = $derived(
        $pendingConfirmation ? Math.max(0, $pendingConfirmation.expiresAt - now) : 0,
    );
    let remainingSeconds = $derived(Math.ceil(remainingMs / 1000));
    let remainingFraction = $derived(
        Math.max(0, Math.min(1, remainingMs / CONFIRMATION_EXPIRY_MS)),
    );
</script>

{#if $pendingConfirmation}
    <div
        class="vcf-card vcf-confirm"
        role="status"
        aria-live="assertive"
        aria-label="Voice command confirmation"
    >
        <div class="vcf-row">
            <AlertTriangle class="vcf-icon vcf-icon-warn" aria-hidden="true" />
            <div class="vcf-text">
                <div class="vcf-title">{$pendingConfirmation.title}</div>
                <div class="vcf-sub">
                    Say <strong>“confirm”</strong> or <strong>“cancel”</strong>
                    <span class="vcf-secs">· {remainingSeconds}s</span>
                </div>
            </div>
        </div>
        <div class="vcf-countdown" aria-hidden="true">
            <span
                class="vcf-countdown-fill"
                style:width="{remainingFraction * 100}%"
            ></span>
        </div>
        <div class="vcf-actions">
            <button
                type="button"
                class="vcf-btn vcf-btn-confirm"
                onclick={() => void confirmPending()}
            >
                <Check class="vcf-btn-icon" aria-hidden="true" />
                Confirm
            </button>
            <button
                type="button"
                class="vcf-btn vcf-btn-cancel"
                onclick={() => cancelPending()}
            >
                <X class="vcf-btn-icon" aria-hidden="true" />
                Cancel
            </button>
        </div>
    </div>
{:else if $voiceFeedback}
    {#key $voiceFeedback.at}
        <div
            class="vcf-card vcf-feedback vcf-tone-{$voiceFeedback.tone}"
            role="status"
            aria-live="polite"
        >
            <Mic class="vcf-icon" aria-hidden="true" />
            <div class="vcf-text">
                <div class="vcf-heard">“{$voiceFeedback.heard}”</div>
                <div class="vcf-detail">{$voiceFeedback.detail}</div>
            </div>
        </div>
    {/key}
{/if}

<style>
    /* Floating card centered above the 28px status bar. Both variants
       share geometry; the confirmation variant adds the actions row. */
    .vcf-card {
        position: fixed;
        bottom: 38px;
        left: 50%;
        transform: translateX(-50%);
        z-index: 45;
        width: min(360px, calc(100vw - 32px));
        box-sizing: border-box;
        padding: 10px 12px;
        border-radius: 12px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        box-shadow: 0 8px 28px rgba(0, 0, 0, 0.32);
        animation: vcf-in 160ms ease-out;
    }

    @keyframes vcf-in {
        from {
            opacity: 0;
            transform: translateX(-50%) translateY(10px);
        }
        to {
            opacity: 1;
            transform: translateX(-50%) translateY(0);
        }
    }

    .vcf-row {
        display: flex;
        align-items: flex-start;
        gap: 9px;
    }

    /* Feedback variant is purely informational — never intercept
       clicks meant for the app beneath it. */
    .vcf-feedback {
        display: flex;
        align-items: flex-start;
        gap: 9px;
        pointer-events: none;
    }

    .vcf-text {
        min-width: 0;
        flex: 1;
    }

    :global(.vcf-icon) {
        width: 16px;
        height: 16px;
        flex-shrink: 0;
        margin-top: 1px;
        color: var(--color-text-secondary);
    }
    :global(.vcf-icon-warn) {
        color: var(--color-warning);
    }

    .vcf-title {
        font-size: 12.5px;
        font-weight: 600;
        line-height: 1.3;
    }

    .vcf-sub {
        margin-top: 2px;
        font-size: 11px;
        color: var(--color-text-secondary);
    }
    .vcf-sub strong {
        font-weight: 600;
        color: inherit;
    }
    .vcf-secs {
        font-variant-numeric: tabular-nums;
        opacity: 0.85;
    }

    /* Heard transcript — quoted, quiet; the detail line carries tone. */
    .vcf-heard {
        font-size: 11px;
        color: var(--color-text-secondary);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .vcf-detail {
        margin-top: 2px;
        font-size: 12.5px;
        font-weight: 600;
        line-height: 1.3;
    }
    .vcf-tone-success .vcf-detail {
        color: var(--color-success);
    }
    .vcf-tone-error .vcf-detail {
        color: var(--color-error);
    }
    .vcf-tone-info .vcf-detail {
        color: var(--color-accent);
    }
    .vcf-tone-success {
        border-color: color-mix(in srgb, var(--color-success) 45%, var(--color-border));
    }
    .vcf-tone-error {
        border-color: color-mix(in srgb, var(--color-error) 45%, var(--color-border));
    }

    /* Confirmation variant — warning-toned so it reads as "decide". */
    .vcf-confirm {
        border-color: color-mix(in srgb, var(--color-warning) 55%, var(--color-border));
    }

    /* Countdown to the auto-expiry — the fill shrinks left-to-right. */
    .vcf-countdown {
        margin-top: 8px;
        height: 3px;
        border-radius: 999px;
        background: color-mix(in srgb, var(--color-warning) 18%, transparent);
        overflow: hidden;
    }
    .vcf-countdown-fill {
        display: block;
        height: 100%;
        background: var(--color-warning);
        transition: width 150ms linear;
    }

    .vcf-actions {
        margin-top: 9px;
        display: flex;
        gap: 7px;
    }

    .vcf-btn {
        flex: 1;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 5px;
        padding: 5px 10px;
        border-radius: 8px;
        font-size: 11.5px;
        font-weight: 600;
        cursor: pointer;
        border: 1px solid var(--color-border);
        transition:
            background-color 140ms ease,
            border-color 140ms ease;
    }
    :global(.vcf-btn-icon) {
        width: 13px;
        height: 13px;
    }

    /* Confirm runs a dangerous command — deliberate, warning-toned,
       never an inviting "go" green. Cancel is the calm safe default. */
    .vcf-btn-confirm {
        color: var(--color-warning);
        background: color-mix(in srgb, var(--color-warning) 14%, transparent);
        border-color: color-mix(in srgb, var(--color-warning) 45%, var(--color-border));
    }
    .vcf-btn-confirm:hover {
        background: color-mix(in srgb, var(--color-warning) 26%, transparent);
    }
    .vcf-btn-cancel:hover {
        background: color-mix(in srgb, var(--color-muted) 18%, transparent);
        border-color: color-mix(in srgb, var(--color-muted) 50%, var(--color-border));
    }
</style>
