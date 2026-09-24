<script lang="ts">
    import { onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { _ } from 'svelte-i18n';
    // The one brand mark used everywhere (logo-mark.png == workspace.png).
    // The theme-conditional light variant was retired 2026-07-16; the splash
    // paints before the theme is known and now uses the same single asset as
    // Logo.svelte does on every theme.
    import logoMark from '$lib/assets/brand/logo-mark.png';

    let fading = $state(false);

    function sleep(ms: number) {
        return new Promise((r) => setTimeout(r, ms));
    }

    onMount(async () => {
        // Content is shown INSTANTLY (no entrance stagger). Hold the splash
        // briefly as a branding moment + to let backend init run, then fade out.
        await sleep(1000);
        await invoke('set_ready', { task: 'frontend' });
        fading = true;
        // Wait for the exit transition (~360ms) before signaling backend to close.
        await sleep(360);
        await invoke('set_ready', { task: 'backend' });
    });
</script>

<!--
  Splash redesign — full-bleed (edge-to-edge), calm, on the design system.
  An opaque var(--color-bg) canvas with a soft center lift for depth (no card
  frame), the real brand mark shown bare with a soft accent halo, the wordmark,
  tagline, ONE motion cue (a slim indeterminate progress bar), and a muted
  status line. Replaces the old four competing animations (pulsing ambient +
  pulsing glow + rotating ring + glowing bouncing dots).

  Colors use design tokens with dark-theme fallbacks (the splash paints before
  `data-theme` is guaranteed). The canvas is opaque from the first frame — a
  transparent body can flash the desktop/white on cold startup.

  Flicker-safe (overlay rule): the canvas AND its content mount visible — shown
  instantly with no entrance stagger; the whole thing fades out on exit.
-->
<div class="wrap {fading ? 'fade-out' : ''}">
    <div class="logo" aria-hidden="true">
        <img src={logoMark} alt="KeepItLocal" />
    </div>

    <div class="wordmark">KeepItLocal</div>

    <div class="tagline">
        <span class="dot" aria-hidden="true"></span>
        {$_('overlay.splash.tagline')}
        <span class="dot" aria-hidden="true"></span>
    </div>

    <div class="progress" aria-hidden="true"><span></span></div>

    <div class="status">{$_('overlay.splash.status')}</div>
</div>

<style>
    /* Opaque canvas from frame 1 — a transparent body can flash the desktop or
       white on cold startup before the webview's first paint. The fallback
       equals the dark-theme token so it's correct even before `data-theme`. */
    :global(html, body) {
        margin: 0;
        height: 100%;
        overflow: hidden;
        background: var(--color-bg, #0c0c0e);
        font-family: 'Inter', system-ui, -apple-system, 'Segoe UI', sans-serif;
        -webkit-font-smoothing: antialiased;
        -moz-osx-font-smoothing: grayscale;
    }

    /* Full-bleed: the canvas IS the splash — no card frame. A faint center lift
       (panel→bg radial) adds depth without a border. */
    .wrap {
        position: relative;
        height: 100%;
        width: 100%;
        box-sizing: border-box;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 14px;
        padding: 24px;
        color: var(--color-text, #e5e5e5);
        background: radial-gradient(
            ellipse 72% 72% at 50% 42%,
            var(--color-panel, #151517) 0%,
            var(--color-bg, #0c0c0e) 70%
        );
        transition:
            opacity 360ms var(--ease-out, cubic-bezier(0.16, 1, 0.3, 1)),
            transform 360ms var(--ease-out, cubic-bezier(0.16, 1, 0.3, 1));
    }
    .wrap.fade-out {
        opacity: 0;
        transform: scale(1.015);
        transition:
            opacity 340ms cubic-bezier(0.4, 0, 1, 1),
            transform 340ms cubic-bezier(0.4, 0, 1, 1);
    }

    /* ─── Brand mark — shown bare (matches Logo.svelte everywhere else), with a
       soft static accent halo behind it instead of a boxed tile. ─────────── */
    .logo {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .logo::before {
        content: '';
        position: absolute;
        inset: -30px;
        border-radius: 50%;
        background: radial-gradient(
            circle,
            color-mix(in srgb, var(--color-accent, #10b981) 20%, transparent) 0%,
            transparent 62%
        );
        pointer-events: none;
    }
    .logo img {
        position: relative;
        height: 72px;
        width: auto;
        object-fit: contain;
        display: block;
    }

    /* ─── Wordmark ──────────────────────────────────────────────────────── */
    .wordmark {
        position: relative;
        font-size: 20px;
        font-weight: 600;
        letter-spacing: -0.02em;
        color: var(--color-text, #e5e5e5);
    }

    /* ─── Tagline — hairline accent dots either side ────────────────────── */
    .tagline {
        position: relative;
        display: flex;
        align-items: center;
        gap: 9px;
        font-size: 10.5px;
        text-transform: uppercase;
        letter-spacing: 0.16em;
        color: var(--color-text-secondary, #a3a3a3);
    }
    .tagline .dot {
        width: 3px;
        height: 3px;
        border-radius: 50%;
        background: var(--color-accent, #10b981);
    }

    /* ─── Indeterminate progress bar — the single, calm "loading" cue ────── */
    .progress {
        position: relative;
        width: 150px;
        height: 3px;
        margin-top: 4px;
        border-radius: 999px;
        background: color-mix(in srgb, var(--color-accent, #10b981) 14%, var(--color-panel-2, #1d1d20));
        overflow: hidden;
    }
    .progress span {
        position: absolute;
        top: 0;
        left: -40%;
        height: 100%;
        width: 40%;
        border-radius: inherit;
        background: var(--color-accent, #10b981);
        animation: slide 1.15s ease-in-out infinite;
    }
    /* ─── Status caption — very small, very muted ───────────────────────── */
    .status {
        position: relative;
        font-size: 9.5px;
        text-transform: uppercase;
        letter-spacing: 0.18em;
        color: var(--color-muted, #737373);
    }

    @keyframes slide {
        0% {
            left: -40%;
        }
        100% {
            left: 100%;
        }
    }

    /* Respect reduced motion — content at rest, progress a static filled bar,
       no slide / entrance / exit movement. */
    @media (prefers-reduced-motion: reduce) {
        .wrap,
        .wrap.fade-out {
            transition: opacity 200ms linear;
            transform: none;
        }
        .progress span {
            animation: none;
            left: 0;
            width: 100%;
            opacity: 0.65;
        }
    }
</style>
