<script lang="ts">
    /*
      Inline failure UI for a single tool screen. Rendered by the
      <svelte:boundary> in +page.svelte's `failed` snippet — so a
      render-time crash in one tool shows here instead of tearing
      down the whole app shell (TitleBar / Sidebar / StatusBar stay
      live and interactive). The full-screen GlobalErrorScreen is the
      catastrophic counterpart; this is the isolated-tool counterpart.

      Two recovery paths:
        - "Reset tool" calls the boundary's reset() — remounts the
          crashed screen fresh, clearing its component-local state.
        - "Copy details" puts the error + stack on the clipboard so
          the user can paste it into a bug report (offline-friendly;
          the app makes no network requests of its own).
    */
    import { onMount } from 'svelte';
    import { AlertTriangle, ClipboardCopy, RotateCcw } from '@lucide/svelte';
    import { _ } from 'svelte-i18n';
    import { recordError } from '$lib/stores/errorLog';
    import { toast } from '$lib/stores/toasts';
    import { Button } from '$lib/ui';

    let {
        error,
        reset,
        screenId,
        screenName,
    }: {
        error: unknown;
        reset: () => void;
        screenId: string;
        screenName?: string;
    } = $props();

    /** Best-effort message extraction — thrown values aren't always Errors. */
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

    // Errors caught by <svelte:boundary> are swallowed by Svelte before
    // they reach window.onerror, so installGlobalErrorCapture() never
    // sees them. Record here so a tool crash still lands in the on-disk
    // diagnostic log (Settings → Diagnostics).
    onMount(() => {
        recordError(
            `frontend:tool:${screenId}`,
            message || $_('errors.unknownError'),
            stack || null,
        );
    });

    async function copyDetails() {
        const label = screenName ? `${screenName} (${screenId})` : screenId;
        const text = `Tool: ${label}\nError: ${message || 'Unknown error'}${stack ? `\n\n${stack}` : ''}`;
        try {
            await navigator.clipboard.writeText(text);
            toast($_('errors.toolErrorCopied'), 'success');
        } catch {
            toast($_('errors.toolErrorCopyFailed'), 'error');
        }
    }
</script>

<div class="p-6 max-w-3xl">
    <div class="rounded-lg border border-error-strong bg-error-soft p-5">
        <div class="flex items-start gap-3">
            <div class="shrink-0 text-error" aria-hidden="true">
                <AlertTriangle class="w-5 h-5" />
            </div>
            <div class="min-w-0 flex-1">
                <h2 class="text-base font-semibold text-text">
                    {$_('errors.toolErrorTitle')}
                </h2>
                <p class="mt-1 text-sm text-text-secondary">
                    {$_('errors.toolErrorLead')}
                </p>

                <div class="mt-3 rounded border border-border bg-bg p-3">
                    <div class="text-[10px] font-semibold uppercase tracking-wider text-muted">
                        {screenName ?? screenId} · {$_('errors.errorLabel')}
                    </div>
                    <div class="mt-1 break-words font-mono text-xs text-text">
                        {message || $_('errors.unknownError')}
                    </div>
                    {#if stack}
                        <details class="mt-2">
                            <summary class="cursor-pointer text-xs text-text-secondary hover:text-text">
                                {$_('errors.stackTrace')}
                            </summary>
                            <pre class="mt-1.5 max-h-60 overflow-y-auto whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed text-muted">{stack}</pre>
                        </details>
                    {/if}
                </div>

                <div class="mt-3 flex flex-wrap gap-2">
                    <Button variant="primary" size="sm" icon={RotateCcw} onclick={reset}>
                        {$_('errors.resetTool')}
                    </Button>
                    <Button variant="secondary" size="sm" icon={ClipboardCopy} onclick={copyDetails}>
                        {$_('errors.reportToolError')}
                    </Button>
                </div>
            </div>
        </div>
    </div>
</div>
