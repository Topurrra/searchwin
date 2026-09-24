<script lang="ts">
    /*
      Custom window chrome for the main KeepItLocal window. Replaces the OS
      title bar (disabled in tauri.conf.json via `decorations: false`) with
      a slim, theme-aware bar that matches the rest of the design language.

      Why not keep the OS chrome:
        - On Windows the system title bar is a fixed off-white/dark slab
          that doesn't follow our theme tokens — looks bolted on next to
          Linear-style panels.
        - Custom chrome lets us put app branding (icon + name) inline with
          window controls and align everything to the design system.

      What we get for free from Tauri:
        - `data-tauri-drag-region` on any element makes it drag the window
        - Double-click on a drag-region element toggles maximize
        - Aero Snap from drag still works (Win key + arrow, etc.)

      What we have to implement:
        - Min / Max / Close buttons calling the webview window APIs
        - Maximize <-> Restore icon swap based on window state
        - Live state listener so the icon flips when user maximizes via
          Win+Up or by dragging to the top of screen

      We deliberately DO NOT add custom resize handles — Tauri leaves the
      OS-level edge resize working even with decorations off on Windows,
      so dragging the edge still resizes. We just lose the rendered chrome.
    */
    import { onMount, onDestroy } from 'svelte';
    import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
    import { Minus, Square, Copy as CopyIcon, X } from '@lucide/svelte';
    import type { UnlistenFn } from '@tauri-apps/api/event';
    import { _ } from 'svelte-i18n';
    import Logo from './Logo.svelte';

    /** True when the window is currently maximized. Drives the icon flip
     * between "maximize" and "restore" — Windows convention. */
    let maximized = $state(false);
    let unlistenResize: UnlistenFn | null = null;

    const win = getCurrentWebviewWindow();

    async function syncMaximizedState() {
        try {
            maximized = await win.isMaximized();
        } catch {
            // No-op: a "isMaximized failed" doesn't matter for chrome
            // rendering — we just default to the maximize icon and the
            // user clicks again if state is stale.
        }
    }

    onMount(async () => {
        await syncMaximizedState();
        // Listen for resize events so the icon flips when the user
        // maximizes via Win+Up or by dragging the window to the top
        // edge (Aero Snap). Without this the button keeps showing
        // "maximize" even though the window already is maximized.
        unlistenResize = await win.onResized(() => {
            void syncMaximizedState();
        });
    });

    onDestroy(() => {
        if (unlistenResize) unlistenResize();
    });

    async function minimize() {
        try {
            await win.minimize();
        } catch (error) {
            console.warn('minimize failed:', error);
        }
    }

    async function toggleMax() {
        try {
            await win.toggleMaximize();
            // The onResized listener will update `maximized`, but a fast
            // explicit refresh keeps the icon responsive on slow systems.
            await syncMaximizedState();
        } catch (error) {
            console.warn('toggleMaximize failed:', error);
        }
    }

    async function closeWindow() {
        try {
            await win.close();
        } catch (error) {
            console.warn('close failed:', error);
        }
    }
</script>

<header class="title-bar" data-tauri-drag-region>
    <!-- App identity — the single canonical place the KeepItLocal mark
         lives now (moved here from the Sidebar / TopBar). Drag region is
         the parent, so the icon + title both act as draggable area;
         clicking them just drags the window. -->
    <div class="brand" data-tauri-drag-region>
        <span class="brand-icon" data-tauri-drag-region aria-hidden="true">
            <Logo size={22} />
        </span>
        <span class="brand-title" data-tauri-drag-region
            >Keep <span class="brand-it" data-tauri-drag-region>IT</span> Local<span
                class="brand-product"
                data-tauri-drag-region>WORKSPACE</span
            ></span
        >
    </div>

    <!-- Window controls. Buttons MUST NOT have `data-tauri-drag-region`
         or they'd drag instead of click. The middle area between brand
         and controls IS draggable (covered by the parent's region). -->
    <div class="controls">
        <button
            class="ctrl"
            type="button"
            onclick={() => void minimize()}
            aria-label={$_('nav.minimize')}
            title={$_('nav.minimize')}
        >
            <Minus class="ctrl-icon" />
        </button>
        <button
            class="ctrl"
            type="button"
            onclick={() => void toggleMax()}
            aria-label={maximized ? $_('nav.restore') : $_('nav.maximize')}
            title={maximized ? $_('nav.restore') : $_('nav.maximize')}
        >
            {#if maximized}
                <!-- Two overlapping squares = restore (Windows convention) -->
                <CopyIcon class="ctrl-icon ctrl-restore-icon" />
            {:else}
                <Square class="ctrl-icon" />
            {/if}
        </button>
        <button
            class="ctrl ctrl-close"
            type="button"
            onclick={() => void closeWindow()}
            aria-label={$_('nav.close')}
            title={$_('nav.close')}
        >
            <X class="ctrl-icon" />
        </button>
    </div>
</header>

<style>
    .title-bar {
        height: 36px;
        flex-shrink: 0;
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding-left: 12px;
        background: var(--color-panel);
        border-bottom: 1px solid var(--color-border);
        user-select: none;
        /* `-webkit-app-region: drag` is the legacy attribute some Tauri
           runtimes still respect — `data-tauri-drag-region` is the modern
           one. Setting both makes the drag work regardless of which
           internal path Tauri uses on the current target. */
        -webkit-app-region: drag;
    }

    .brand {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        flex: 1;
        min-width: 0;
    }

    /* Logo mark — the KeepItLocal chevron (Logo.svelte). The leading
       chevron is theme-accent tinted, so the mark recolors with the active
       theme. Wrapper just centers + sizes the SVG. */
    .brand-icon {
        width: 22px;
        height: 22px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
    }

    .brand-title {
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
        letter-spacing: -0.008em;
    }
    .brand-it {
        /* Logo red, not the user accent — the brand mark's crimson. */
        color: var(--color-brand-red);
    }
    /* Sub-product modifier — "WORKSPACE" lives next to the umbrella
       brand "Keep IT Local". Gray + uppercase + tracked-out so it
       reads as "this is the Workspace product in the KeepItLocal
       family", quietly. Per the user's Keep IT Local brand family:
       Workspace (desktop), Guard (extension), Vault (extension).
       Only the IT stays accent — the sub-product modifier is muted. */
    .brand-product {
        margin-left: 6px;
        padding: 1px 6px;
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.1em;
        color: var(--color-muted);
        text-transform: uppercase;
        border-radius: 4px;
        background: var(--color-panel-2);
        vertical-align: 1px;
    }

    .controls {
        display: inline-flex;
        align-items: center;
        -webkit-app-region: no-drag;
    }

    .ctrl {
        width: 46px;
        height: 36px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border: none;
        background: transparent;
        color: var(--color-text-secondary);
        cursor: pointer;
        transition: background-color 120ms ease, color 120ms ease;
        /* Explicit `no-drag` on each button so a single bad attribute
           inheritance doesn't accidentally turn a button into draggable
           dead zone. */
        -webkit-app-region: no-drag;
    }

    .ctrl:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }

    /* Close button gets a destructive accent on hover — matches the
       Windows 11 convention so muscle memory still works. */
    .ctrl-close:hover {
        background: var(--color-error, #fb7185);
        color: #ffffff;
    }

    .ctrl:focus-visible {
        outline: 2px solid color-mix(in srgb, var(--color-accent) 55%, transparent);
        outline-offset: -2px;
    }

    :global(.ctrl-icon) {
        width: 14px;
        height: 14px;
    }

    /* Restore icon is the lucide CopyIcon (two overlapping squares).
       It's slightly large — scale down + rotate so it reads as a
       restore-window glyph rather than a clipboard. */
    :global(.ctrl-restore-icon) {
        width: 12px;
        height: 12px;
    }
</style>
