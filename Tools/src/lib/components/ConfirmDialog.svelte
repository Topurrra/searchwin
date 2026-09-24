<script lang="ts">
    /*
      ConfirmDialog — singleton modal driven by the `activeConfirm`
      store. Mounted once per window in +layout.svelte; renders the
      currently-active dialog (or nothing) reactively.

      Visual tone matches the app's design tokens — same panel
      background, border radius, accent color, kbd-chip style as the
      overlay surfaces. Themes flow through automatically via CSS
      vars; switching dark→light flips the dialog with the rest of
      the UI.

      Keyboard:
        - Esc cancels (calls onCancel)
        - Enter confirms (calls onConfirm), UNLESS `danger` is set —
          in that case the cancel button is focused and Enter still
          confirms IF it's the focused control, but the default
          focus is "safe".

      Backdrop click cancels.
    */
    import { onMount } from 'svelte';
    import { AlertTriangle, AlertCircle, HelpCircle, Info, X } from '@lucide/svelte';
    import { activeConfirm } from '$lib/stores/confirmDialog';
    import { _ } from 'svelte-i18n';

    let confirmButtonEl = $state<HTMLButtonElement | null>(null);
    let cancelButtonEl = $state<HTMLButtonElement | null>(null);

    /** Remember the element that had focus before the dialog opened
     *  so we can return focus there on close. Without this the
     *  caller's focus context is lost when the dialog dismisses. */
    let previousFocus: HTMLElement | null = null;

    /** Auto-focus the right button each time a new dialog appears.
     *  Danger-style dialogs focus Cancel (so an accidental Enter
     *  doesn't fire the destructive action); benign ones focus
     *  Confirm (so Enter is the natural way through). */
    $effect(() => {
        const dialog = $activeConfirm;
        if (!dialog) return;
        // Defer to the next microtask so the bind: refs have landed.
        queueMicrotask(() => {
            if (typeof document !== 'undefined') {
                previousFocus = document.activeElement as HTMLElement | null;
            }
            if (dialog.danger) {
                cancelButtonEl?.focus();
            } else {
                confirmButtonEl?.focus();
            }
        });
    });

    /** When the dialog goes away, return focus to whatever had it
     *  before we stole focus. Mirrors native dialog behavior. */
    $effect(() => {
        if ($activeConfirm) return;
        if (previousFocus && typeof previousFocus.focus === 'function') {
            try {
                previousFocus.focus();
            } catch {
                // Element may have been removed from the DOM — ignore.
            }
        }
        previousFocus = null;
    });

    function onKeydown(event: KeyboardEvent) {
        const dialog = $activeConfirm;
        if (!dialog) return;
        if (event.key === 'Escape') {
            event.preventDefault();
            dialog.onCancel();
            return;
        }
        if (event.key === 'Enter') {
            // Skip when the user is composing IME text — pressing
            // Enter mid-composition should commit the composition,
            // not fire the dialog action.
            if (event.isComposing) return;
            // Capture-phase listener: we run BEFORE the focused
            // button's click handler. preventDefault stops the
            // browser-default "click the focused button" so we don't
            // double-fire when Confirm/Cancel is focused. We then
            // route Enter to confirm regardless of focus — that's
            // what the footer hint promises, and it's how the user
            // expects keyboard-only flow to work.
            //
            // Safety note: for danger-style dialogs we DEFAULT-focus
            // Cancel (see the focus $effect above). The user still
            // has to either Tab to Confirm + Space, or press Enter
            // intentionally — both of which are deliberate. The
            // visual red Confirm button + the warning icon set the
            // expectation; Enter being universal keeps it predictable.
            event.preventDefault();
            dialog.onConfirm();
        }
    }

    onMount(() => {
        // Component-scoped keydown listener so Esc works no matter
        // which element has focus inside the dialog.
        if (typeof window !== 'undefined') {
            window.addEventListener('keydown', onKeydown, { capture: true });
            return () => window.removeEventListener('keydown', onKeydown, { capture: true });
        }
    });
</script>

{#if $activeConfirm}
    {@const dialog = $activeConfirm}
    <div
        class="cd-backdrop"
        onclick={(event) => { if (event.target === event.currentTarget) dialog.onCancel(); }}
        role="presentation"
    >
        <div
            class="cd-card cd-kind-{dialog.kind}"
            role="dialog"
            aria-modal="true"
            aria-labelledby="cd-title"
            aria-describedby="cd-message"
            tabindex="-1"
        >
            <button
                type="button"
                class="cd-close"
                onclick={() => dialog.onCancel()}
                title={$_('errors.closeEsc')}
                aria-label={$_('errors.closeDialog')}
            >
                <X class="w-3.5 h-3.5" />
            </button>

            <div class="cd-header">
                <span class="cd-icon" aria-hidden="true">
                    {#if dialog.kind === 'warning'}
                        <AlertTriangle class="w-5 h-5" />
                    {:else if dialog.kind === 'error'}
                        <AlertCircle class="w-5 h-5" />
                    {:else if dialog.kind === 'info'}
                        <Info class="w-5 h-5" />
                    {:else}
                        <HelpCircle class="w-5 h-5" />
                    {/if}
                </span>
                <h2 id="cd-title" class="cd-title">{dialog.title}</h2>
            </div>

            <p id="cd-message" class="cd-message">{dialog.message}</p>

            <div class="cd-actions">
                <button
                    type="button"
                    class="cd-btn cd-btn-cancel"
                    onclick={() => dialog.onCancel()}
                    bind:this={cancelButtonEl}
                >
                    {dialog.cancelLabel}
                </button>
                <button
                    type="button"
                    class="cd-btn cd-btn-confirm"
                    class:cd-btn-danger={dialog.danger}
                    onclick={() => dialog.onConfirm()}
                    bind:this={confirmButtonEl}
                >
                    {dialog.confirmLabel}
                </button>
            </div>

            <div class="cd-footer">
                <span><kbd>Esc</kbd> {$_('errors.kbdCancel')} · <kbd>Enter</kbd> {$_('errors.kbdConfirm')}</span>
            </div>
        </div>
    </div>
{/if}

<style>
    /* Backdrop covers the whole window with a subtle dimming. Click
       anywhere outside the card to cancel — same convention as the
       search/clipboard overlays. */
    .cd-backdrop {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.45);
        z-index: 9999;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 24px;
    }

    /* Card uses the same panel/border/radius tokens as the rest of
       the app so it reads as a KeepItLocal surface, not an OS dialog. */
    .cd-card {
        position: relative;
        max-width: 420px;
        width: 100%;
        background: var(--color-panel, #141414);
        border: 1px solid var(--color-border, #262626);
        border-radius: 16px;
        padding: 18px 20px 14px 20px;
        box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45),
                    0 4px 12px rgba(0, 0, 0, 0.25);
        display: flex;
        flex-direction: column;
        gap: 12px;
    }

    /* Close button — top-right, low-emphasis. Esc is the primary
       dismiss path; this is just for mouse users. */
    .cd-close {
        position: absolute;
        top: 8px;
        right: 8px;
        width: 24px;
        height: 24px;
        border-radius: 6px;
        border: 1px solid transparent;
        background: transparent;
        color: var(--color-muted, #737373);
        display: inline-flex;
        align-items: center;
        justify-content: center;
        cursor: pointer;
        transition: background-color 140ms ease, color 140ms ease,
                    border-color 140ms ease;
    }
    .cd-close:hover {
        background: var(--color-panel-2, #1c1c1c);
        color: var(--color-text, #e5e5e5);
        border-color: var(--color-border, #262626);
    }

    /* Header — kind-specific icon + title. Icon color encodes the
       semantic (warning amber, error red, info blue, question accent). */
    .cd-header {
        display: flex;
        align-items: center;
        gap: 10px;
        padding-right: 28px; /* leave room for the close button */
    }
    .cd-icon {
        width: 36px;
        height: 36px;
        border-radius: 10px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
    }
    .cd-kind-warning .cd-icon {
        color: var(--color-warning, #f59e0b);
        background: color-mix(in srgb, var(--color-warning, #f59e0b) 14%, transparent);
    }
    .cd-kind-error .cd-icon {
        color: var(--color-error, #ef4444);
        background: color-mix(in srgb, var(--color-error, #ef4444) 14%, transparent);
    }
    .cd-kind-info .cd-icon {
        color: var(--color-info, #3b82f6);
        background: color-mix(in srgb, var(--color-info, #3b82f6) 14%, transparent);
    }
    .cd-kind-question .cd-icon {
        color: var(--color-accent, #10b981);
        background: color-mix(in srgb, var(--color-accent, #10b981) 14%, transparent);
    }
    .cd-title {
        margin: 0;
        font-size: 15px;
        font-weight: 600;
        color: var(--color-text, #e5e5e5);
        line-height: 1.3;
    }

    .cd-message {
        margin: 0;
        font-size: 13px;
        color: var(--color-text-secondary, #a3a3a3);
        line-height: 1.5;
        white-space: pre-wrap;
    }

    /* Actions — cancel ghost on the left, confirm filled on the right.
       Confirm flips to error-red when `danger` is set, signaling the
       destructive nature without relying on copy. */
    .cd-actions {
        display: flex;
        justify-content: flex-end;
        gap: 8px;
        margin-top: 4px;
    }
    .cd-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        height: 34px;
        padding: 0 14px;
        border-radius: 8px;
        font-size: 13px;
        font-weight: 500;
        cursor: pointer;
        transition: background-color 140ms ease, border-color 140ms ease,
                    color 140ms ease, transform 100ms ease;
        border: 1px solid transparent;
    }
    .cd-btn:active:not(:disabled) {
        transform: translateY(1px);
    }
    .cd-btn-cancel {
        background: transparent;
        border-color: var(--color-border, #262626);
        color: var(--color-text-secondary, #a3a3a3);
    }
    .cd-btn-cancel:hover {
        background: var(--color-panel-2, #1c1c1c);
        color: var(--color-text, #e5e5e5);
        border-color: color-mix(in srgb, var(--color-accent, #10b981) 35%, var(--color-border, #262626));
    }
    .cd-btn-confirm {
        background: var(--color-accent, #10b981);
        color: var(--color-accent-contrast, #0a0a0a);
        border-color: var(--color-accent, #10b981);
    }
    .cd-btn-confirm:hover {
        background: var(--color-accent-hover, var(--color-accent, #10b981));
        filter: brightness(1.06);
    }
    .cd-btn-danger {
        background: var(--color-error, #ef4444);
        color: #fff;
        border-color: var(--color-error, #ef4444);
    }
    .cd-btn-danger:hover {
        background: var(--color-error, #ef4444);
        filter: brightness(1.08);
    }
    .cd-btn:focus-visible {
        outline: 2px solid var(--color-accent, #10b981);
        outline-offset: 2px;
    }

    /* Footer — tiny kbd hint row. Same tone as the overlay footers. */
    .cd-footer {
        display: flex;
        justify-content: flex-end;
        font-size: 10px;
        color: var(--color-muted, #737373);
        padding-top: 4px;
        border-top: 1px solid var(--color-border, #262626);
    }
    .cd-footer kbd {
        font-family: inherit;
        font-size: 10px;
        padding: 1px 5px;
        margin: 0 2px;
        border-radius: 4px;
        background: var(--color-panel-2, #1c1c1c);
        border: 1px solid var(--color-border, #262626);
        color: var(--color-text-secondary, #a3a3a3);
    }
    /* Neutral desktop-dialog override: retain the proven focus and queue behavior above. */
    .cd-backdrop {
        background: color-mix(in srgb, var(--color-bg) 76%, transparent);
        animation: none;
    }
    .cd-card {
        max-width: 440px;
        padding: 20px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
        box-shadow:
            0 18px 40px color-mix(in srgb, var(--color-text) 16%, transparent),
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 7%, transparent);
        gap: 14px;
        animation: none;
    }
    .cd-card::before {
        position: absolute;
        top: 18px;
        bottom: 18px;
        left: 0;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }
    .cd-kind-warning.cd-card::before { background: var(--color-warning); }
    .cd-kind-error.cd-card::before { background: var(--color-error); }
    .cd-kind-info.cd-card::before { background: var(--color-info); }

    .cd-close {
        border-radius: var(--radius-control, 8px);
        color: var(--color-muted);
        transition: background-color var(--dur-micro) var(--ease-out), color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out);
    }
    .cd-close:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
        border-color: var(--color-border);
    }

    .cd-header { gap: 12px; }
    .cd-icon {
        width: 38px;
        height: 38px;
        border-radius: 10px;
        background: transparent;
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .cd-kind-warning .cd-icon { color: var(--color-warning); background: transparent; }
    .cd-kind-error .cd-icon { color: var(--color-error); background: transparent; }
    .cd-kind-info .cd-icon { color: var(--color-info); background: transparent; }
    .cd-kind-question .cd-icon { color: var(--color-accent); background: transparent; }
    .cd-title { color: var(--color-text); }
    .cd-message { color: var(--color-text-secondary); }

    .cd-actions {
        margin-top: 2px;
        padding-top: 14px;
        border-top: 1px solid var(--color-border);
    }
    .cd-btn {
        border-radius: var(--radius-control, 8px);
        transition: background-color var(--dur-micro) var(--ease-out), border-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .cd-btn:active:not(:disabled) { transform: none; }
    .cd-btn-cancel {
        border-color: color-mix(in srgb, var(--color-text) 10%, transparent);
        color: var(--color-text-secondary);
    }
    .cd-btn-cancel:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
        border-color: color-mix(in srgb, var(--color-text) 18%, var(--color-border));
    }
    .cd-btn-confirm {
        background: var(--color-accent);
        color: var(--color-accent-contrast);
        border-color: var(--color-accent);
    }
    .cd-btn-confirm:hover { background: var(--color-accent-hover); filter: none; }
    .cd-btn-danger {
        background: var(--color-error);
        border-color: var(--color-error);
    }
    .cd-btn-danger:hover {
        background: color-mix(in srgb, var(--color-error) 88%, var(--color-text));
        filter: none;
    }
    .cd-btn:focus-visible { outline-color: var(--color-accent); }

    .cd-footer {
        padding-top: 0;
        border-top: 0;
        color: var(--color-muted);
    }
    .cd-footer kbd {
        background: transparent;
        border-color: color-mix(in srgb, var(--color-text) 12%, transparent);
        color: var(--color-text-secondary);
    }

    @media (max-width: 520px) {
        .cd-backdrop { padding: 16px; }
        .cd-card { padding: 18px; }
        .cd-actions { align-items: stretch; flex-direction: column-reverse; }
        .cd-btn { width: 100%; }
    }
    @media (prefers-reduced-motion: reduce) {
        .cd-close, .cd-btn { transition: none; }
    }
</style>
