<script lang="ts">
    /*
      Last-resort error UI. Replaces the white-screen-of-death that happens
      when an uncaught error explodes the render tree. Mounted by the root
      layout inside an `<svelte:boundary>` — so any render-time error in
      ANY screen falls back to this instead of leaving the user with a
      blank window and no way out.

      Two recovery paths:
        - "Try again" calls the boundary's `reset()` so the user stays in
          the current window and can retry whatever they were doing.
        - "Reload KeepItLocal" hits `location.reload()` which is the nuclear
          option — clears component state but keeps the OS-level app
          process and its hotkey registrations alive.

      Also surfaces the error message + stack to the user. Linear/Notion-
      style "we hit a bug" UI rather than a raw browser error dump.
    */
    import { AlertOctagon, RotateCcw, RefreshCw } from '@lucide/svelte';
    import { _ } from 'svelte-i18n';

    let {
        error,
        reset,
    }: {
        error: unknown;
        reset: () => void;
    } = $props();

    /** Best-effort message extraction. Real Errors have .message; thrown
     * strings/numbers/objects need to be stringified safely. */
    let message = $derived(
        error instanceof Error
            ? error.message
            : typeof error === 'string'
              ? error
              : (() => {
                    try {
                        return JSON.stringify(error);
                    } catch {
                        return String(error);
                    }
                })(),
    );

    let stack = $derived(error instanceof Error ? error.stack ?? '' : '');

    function reload() {
        // Full reload — the simplest way to recover from corrupted
        // module state, leaked listeners, or anything else that
        // accumulated before the crash. Backend process keeps running
        // (Tauri only reloads the webview), so global hotkeys + clipboard
        // listener survive.
        window.location.reload();
    }
</script>

<div class="root">
    <div class="card">
        <div class="icon-tile" aria-hidden="true">
            <AlertOctagon class="error-icon" />
        </div>

        <h1>{$_('errors.somethingWentWrong')}</h1>
        <p class="lead">
            {$_('errors.globalErrorLead')}
        </p>

        <div class="error-box">
            <div class="error-label">{$_('errors.errorLabel')}</div>
            <div class="error-message">{message || $_('errors.unknownError')}</div>
            {#if stack}
                <details>
                    <summary>{$_('errors.stackTrace')}</summary>
                    <pre>{stack}</pre>
                </details>
            {/if}
        </div>

        <div class="actions">
            <button class="primary" type="button" onclick={reset}>
                <RotateCcw class="btn-icon" />
                {$_('errors.tryAgain')}
            </button>
            <button class="secondary" type="button" onclick={reload}>
                <RefreshCw class="btn-icon" />
                {$_('errors.reloadKeepItLocal')}
            </button>
        </div>

        <p class="footnote">
            {$_('errors.globalErrorFootnote')}
        </p>
    </div>
</div>

<style>
    .root {
        position: fixed;
        inset: 0;
        z-index: 9999;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 24px;
        background: var(--color-bg, #0a0a0a);
        color: var(--color-text, #e5e5e5);
    }

    .card {
        max-width: 580px;
        width: 100%;
        padding: 24px 28px 22px;
        border-radius: 16px;
        border: 1px solid var(--color-border, #262626);
        background: var(--color-panel, #141414);
        box-shadow: 0 12px 40px rgba(0, 0, 0, 0.55);
    }

    .icon-tile {
        width: 48px;
        height: 48px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 12px;
        background: color-mix(in srgb, var(--color-error, #fb7185) 18%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-error, #fb7185) 36%, var(--color-border, #262626));
        color: var(--color-error, #fb7185);
        margin-bottom: 16px;
    }

    :global(.error-icon) {
        width: 24px;
        height: 24px;
    }

    h1 {
        margin: 0 0 8px;
        font-size: 20px;
        font-weight: 600;
        letter-spacing: -0.01em;
        color: var(--color-text, #e5e5e5);
    }

    .lead {
        margin: 0 0 18px;
        font-size: 14px;
        line-height: 1.55;
        color: var(--color-text-secondary, #a3a3a3);
    }

    .error-box {
        border-radius: 10px;
        border: 1px solid var(--color-border, #262626);
        background: var(--color-bg, #0a0a0a);
        padding: 12px 14px;
        margin-bottom: 18px;
    }

    .error-label {
        font-size: 10px;
        text-transform: uppercase;
        letter-spacing: 0.08em;
        font-weight: 600;
        color: var(--color-muted, #737373);
        margin-bottom: 6px;
    }

    .error-message {
        font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
        font-size: 12.5px;
        line-height: 1.4;
        color: var(--color-text, #e5e5e5);
        word-wrap: break-word;
    }

    details {
        margin-top: 10px;
    }

    summary {
        cursor: pointer;
        font-size: 11px;
        color: var(--color-text-secondary, #a3a3a3);
        margin-bottom: 6px;
    }

    summary:hover {
        color: var(--color-text, #e5e5e5);
    }

    pre {
        margin: 0;
        font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
        font-size: 11px;
        line-height: 1.45;
        color: var(--color-muted, #737373);
        white-space: pre-wrap;
        word-break: break-word;
        max-height: 240px;
        overflow-y: auto;
    }

    .actions {
        display: flex;
        gap: 8px;
        margin-bottom: 14px;
    }

    button {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 8px 14px;
        border-radius: 9px;
        font-size: 13px;
        font-weight: 600;
        cursor: pointer;
        transition: background-color 140ms ease, border-color 140ms ease;
    }

    button.primary {
        border: 1px solid color-mix(in srgb, var(--color-accent, #10b981) 42%, var(--color-border, #262626));
        background: color-mix(in srgb, var(--color-accent, #10b981) 18%, var(--color-panel, #141414));
        color: var(--color-accent, #10b981);
    }

    button.primary:hover {
        background: color-mix(in srgb, var(--color-accent, #10b981) 26%, var(--color-panel, #141414));
        border-color: color-mix(in srgb, var(--color-accent, #10b981) 65%, var(--color-border, #262626));
    }

    button.secondary {
        border: 1px solid var(--color-border, #262626);
        background: var(--color-panel-2, #1c1c1c);
        color: var(--color-text, #e5e5e5);
    }

    button.secondary:hover {
        border-color: color-mix(in srgb, var(--color-accent, #10b981) 55%, var(--color-border, #262626));
        background: var(--color-panel, #141414);
    }

    :global(.btn-icon) {
        width: 14px;
        height: 14px;
    }

    .footnote {
        margin: 0;
        font-size: 11.5px;
        line-height: 1.5;
        color: var(--color-muted, #737373);
    }
</style>
