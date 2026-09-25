<script lang="ts">
    // Every tool page, in a Search tab. Workspace's window-wide machinery
    // (voice, push-to-talk, hotkeys, its schedulers, its theme and font
    // pickers) stays behind: Search's look comes from styles.css and follows
    // Search's theme by itself.
    import '../styles.css';
    import { onMount, onDestroy } from 'svelte';
    import { initSettingsStore } from '$lib/stores/settings';
    import { initToolPacksStore } from '$lib/stores/toolPacks';
    import GlobalErrorScreen from '$lib/components/GlobalErrorScreen.svelte';
    import ConfirmDialog from '$lib/components/ConfirmDialog.svelte';
    import { setupI18n } from '$lib/i18n';

    // Strings before the first paint; the tools are English only.
    setupI18n('en');

    let { children } = $props();

    /** An error outside the render tree (a promise, a listener): shown the
     *  same way as one inside it, which the boundary below catches. */
    let asyncError = $state<unknown>(null);
    let errorCaptureCleanup: (() => void) | undefined;

    onMount(async () => {
        window.addEventListener('error', (event) => {
            // A picture that didn't load has no error, and isn't one.
            if (event.error) asyncError = event.error;
        });
        window.addEventListener('unhandledrejection', (event) => {
            asyncError = event.reason ?? new Error('Unhandled promise rejection');
        });
        const { installGlobalErrorCapture } = await import('$lib/stores/errorLog');
        errorCaptureCleanup = installGlobalErrorCapture();

        try {
            await initSettingsStore();
            await initToolPacksStore();
            const { initShredderListeners } = await import('$lib/stores/shredder');
            await initShredderListeners();
        } catch (error) {
            asyncError = error;
        }
    });

    onDestroy(() => {
        errorCaptureCleanup?.();
        errorCaptureCleanup = undefined;
    });
</script>

<svelte:boundary>
    {#snippet failed(error, reset)}
        <GlobalErrorScreen {error} {reset} />
    {/snippet}

    {@render children()}
</svelte:boundary>

{#if asyncError}
    <GlobalErrorScreen error={asyncError} reset={() => (asyncError = null)} />
{/if}

<ConfirmDialog />
