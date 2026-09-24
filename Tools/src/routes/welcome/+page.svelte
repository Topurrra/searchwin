<script lang="ts">
    /*
      Welcome route (Wave 7.9, 2026-05-28)
      ────────────────────────────────────
      First-run onboarding lives in its own Tauri window labeled
      "welcome" — see `src-tauri/src/lib.rs` (search WELCOME_WINDOW_LABEL).
      The window is born `maximized: true` + `visible: false` and is
      shown by Rust once this route's first paint is on screen, so the
      transition from splash to welcome is instant: no resize, no dark
      900×700 flash that the in-main approach (pre-7.9) produced.

      All of the onboarding chrome — pillars, privacy promise, packs,
      magic moment, confetti — lives in $lib/WelcomeSetup.svelte. This
      route is intentionally a thin wrapper: it just mounts that
      component and signals readiness back to Rust so the splash hands
      off at the right moment.
    */
    import { onMount, tick } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import WelcomeSetup from '$lib/WelcomeSetup.svelte';
    import { initOnboardingStore } from '$lib/stores/onboarding';
    import { initToolPacksStore } from '$lib/stores/toolPacks';

    let storesReady = $state(false);

    onMount(async () => {
        // Load onboarding + toolPacks state up front so WelcomeSetup
        // can render with real data on its first paint (the pack
        // picker step iterates installedScreens; without this it
        // briefly renders an empty list).
        try {
            await Promise.all([initOnboardingStore(), initToolPacksStore()]);
        } catch (error) {
            console.warn('Welcome: store init failed, continuing with defaults', error);
        }
        storesReady = true;

        // Wait for Svelte to commit the storesReady=true → WelcomeSetup
        // mount → first DOM render cycle BEFORE telling Rust the splash
        // can close (the Rust set_ready handler prefers welcome over
        // main and shows whichever exists, so this is the actual reveal
        // moment for the user). Without the `tick()` + double rAF the
        // splash sometimes closed before WelcomeSetup's heavy imports
        // (canvas-confetti, transitions) committed their first paint,
        // exposing a blank dark frame for a few hundred ms.
        await tick();
        requestAnimationFrame(() => {
            requestAnimationFrame(() => {
                void invoke('set_ready', { task: 'frontend' });
                void invoke('set_ready', { task: 'backend' });
            });
        });
    });
</script>

<svelte:head>
    <title>Welcome to KeepItLocal</title>
</svelte:head>

<div class="welcome-window">
    {#if storesReady}
        <WelcomeSetup />
    {/if}
</div>

<style>
    /* Full-viewport dark backdrop. Welcome's own component owns the
       gradient + content; this div just guarantees the unstyled HTML
       below the component never shows the user-agent default white. */
    .welcome-window {
        position: fixed;
        inset: 0;
        background: #0c0c0e;
        color: #e5e5e5;
        overflow: hidden;
    }
</style>
