<script lang="ts">
    /*
      UI Elements overlay — #20. A transparent, full-screen overlay for
      voice accessibility control: it numbers every clickable control in
      the focused window so the user can click one hands-free.

      How it works: the "show elements" command runs
      `voice_list_ui_elements` FIRST — while the user's app is still the
      foreground window — which enumerates its controls via Windows UI
      Automation and stashes them. This overlay then appears, fetches
      that list with `voice_get_ui_elements`, and badges each control
      with a number. Saying the number (or the control's label) warps
      the cursor there and clicks the app beneath; "cancel" closes.

      Why both numbers AND labels: the command-mode Vosk grammar is
      fixed for a session and cannot hold arbitrary per-app button
      labels — but THIS overlay builds its own recognizer grammar from
      the freshly enumerated labels, so label-speak works here. The
      numbers are the universal fallback for unlabeled, duplicate, or
      icon-only controls.

      Voice: this window runs its OWN grammar-constrained Vosk
      recognizer (numbers + labels + "cancel", source 'ui-elements'),
      acquired through the #17 mic arbiter at priority 35 — it preempts
      command mode, which resumes when the overlay closes.

      Targeting: elements carry screen FRACTIONS [0,1]; the Rust
      `voice_mouse_warp` resolves them to pixels (DPI-aware). The window
      hides itself before the click so it lands on the app underneath.
    */
    import { onMount, onDestroy } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { listen, type UnlistenFn } from '@tauri-apps/api/event';
    import { settings } from '$lib/stores/settings';

    /** One clickable control — mirrors the Rust `UiElement`. */
    interface UiElement {
        label: string;
        fx: number;
        fy: number;
    }

    /** Source label for this window's recognizer — see #17's arbiter. */
    const UI_SOURCE = 'ui-elements';

    /** Spoken number words. `TENS` covers 20–50 (the MAX_ELEMENTS cap). */
    const ONES = [
        'one', 'two', 'three', 'four', 'five',
        'six', 'seven', 'eight', 'nine',
    ];
    const TEENS = [
        'ten', 'eleven', 'twelve', 'thirteen', 'fourteen',
        'fifteen', 'sixteen', 'seventeen', 'eighteen', 'nineteen',
    ];
    const TENS = ['twenty', 'thirty', 'forty', 'fifty'];

    /** The enumerated controls — fetched fresh on every show. */
    let elements = $state<UiElement[]>([]);
    /** True from the moment a click is dispatched until the overlay
     *  resets, so a late voice-final cannot fire a second click. */
    let acting = $state(false);
    /** Digits typed on the keyboard fallback, before Enter confirms. */
    let typed = $state('');

    let unlistenFinal: UnlistenFn | null = null;
    let unlistenReset: UnlistenFn | null = null;
    let unlistenPreempted: UnlistenFn | null = null;

    /** The number words needed to address controls 1..count. */
    function numberWordsUpTo(count: number): string[] {
        const words: string[] = [];
        if (count >= 1) words.push(...ONES.slice(0, Math.min(count, 9)));
        if (count >= 10) words.push(...TEENS.slice(0, Math.min(count - 9, 10)));
        for (let t = 20; t <= count; t += 10) words.push(TENS[(t - 20) / 10]);
        return words;
    }

    /** Parse a spoken number ("twelve", "twenty three") to an integer. */
    function spokenToNumber(words: string[]): number | null {
        if (words.length === 1) {
            const o = ONES.indexOf(words[0]);
            if (o >= 0) return o + 1;
            const t = TEENS.indexOf(words[0]);
            if (t >= 0) return t + 10;
            const d = TENS.indexOf(words[0]);
            if (d >= 0) return (d + 2) * 10;
            return null;
        }
        if (words.length === 2) {
            const d = TENS.indexOf(words[0]);
            const o = ONES.indexOf(words[1]);
            if (d >= 0 && o >= 0) return (d + 2) * 10 + (o + 1);
        }
        return null;
    }

    /** Normalize a label for matching — lowercase, punctuation to
     *  spaces, whitespace collapsed. */
    function normalizeLabel(text: string): string {
        return text
            .toLowerCase()
            .replace(/[^a-z0-9\s]+/g, ' ')
            .replace(/\s+/g, ' ')
            .trim();
    }

    /** The recognizer's full vocabulary for the current element set:
     *  number words + "cancel" + every alphabetic label word. */
    function buildGrammar(els: UiElement[]): string[] {
        const grammar = new Set<string>(['cancel']);
        for (const word of numberWordsUpTo(els.length)) grammar.add(word);
        for (const el of els) {
            for (const word of normalizeLabel(el.label).split(' ')) {
                // Only pure-alphabetic words — a Vosk grammar token must
                // plausibly be in the model's vocabulary; digit / glyph
                // fragments are dropped (the number badge still reaches
                // the control).
                if (word.length > 0 && /^[a-z]+$/.test(word)) {
                    grammar.add(word);
                }
            }
        }
        return [...grammar];
    }

    async function arm() {
        const modelPath = $settings.voskModelPath;
        if (!modelPath || elements.length === 0) return;
        try {
            await invoke('voice_start_continuous', {
                modelPath,
                source: UI_SOURCE,
                grammar: buildGrammar(elements),
            });
        } catch {
            // Mic busy / no model — the keyboard fallback still works.
        }
    }

    /** Hide the overlay window WITHOUT stopping the recognizer — for the
     *  preemption path, where a higher-priority surface already owns
     *  the mic (stopping it would clobber THAT surface's session). */
    async function hideWindow() {
        try {
            await invoke('hide_ui_elements_window_command');
        } catch {
            /* best-effort */
        }
    }

    /** Close the overlay: release the mic, then hide the window. */
    async function closeOverlay() {
        void invoke('voice_stop_continuous').catch(() => {});
        await hideWindow();
    }

    /** Warp the cursor to control `n` (1-based) and click it — hiding
     *  the overlay first so the click lands on the app beneath. */
    async function activate(n: number) {
        if (acting || n < 1 || n > elements.length) return;
        acting = true;
        const el = elements[n - 1];
        void invoke('voice_stop_continuous').catch(() => {});
        await hideWindow();
        // Let the overlay actually disappear before the click is sent.
        await new Promise((r) => setTimeout(r, 140));
        try {
            await invoke('voice_mouse_warp', { fx: el.fx, fy: el.fy });
            await invoke('voice_mouse_click', { button: 'left', double: false });
        } catch {
            /* best-effort */
        }
    }

    function handleFinal(text: string) {
        if (acting) return;
        const cleaned = text.trim().toLowerCase();
        if (cleaned.length === 0 || cleaned.includes('[unk]')) return;
        const words = cleaned.split(/\s+/).filter((w) => w.length > 0);
        if (words.length === 0) return;
        if (words.length === 1 && words[0] === 'cancel') {
            void closeOverlay();
            return;
        }
        const num = spokenToNumber(words);
        if (num !== null) {
            void activate(num);
            return;
        }
        // Label-speak: an EXACT normalized-label match. Exact-only is
        // deliberate — this clicks things, and a loose match could click
        // the wrong control. Duplicate or partial labels fall back to
        // the visible number badge.
        const phrase = normalizeLabel(words.join(' '));
        if (phrase.length === 0) return;
        const hits: number[] = [];
        elements.forEach((el, i) => {
            if (normalizeLabel(el.label) === phrase) hits.push(i + 1);
        });
        if (hits.length === 1) void activate(hits[0]);
        // 0 or ≥2 hits: ignore — the number badge disambiguates.
    }

    async function reset() {
        acting = false;
        typed = '';
        try {
            elements = await invoke<UiElement[]>('voice_get_ui_elements');
        } catch {
            elements = [];
        }
        void arm();
    }

    function onKeydown(event: KeyboardEvent) {
        if (event.key === 'Escape') {
            event.preventDefault();
            void closeOverlay();
        } else if (event.key === 'Enter') {
            event.preventDefault();
            if (typed.length > 0) {
                const n = Number(typed);
                typed = '';
                void activate(n);
            }
        } else if (event.key === 'Backspace') {
            event.preventDefault();
            typed = typed.slice(0, -1);
        } else if (event.key >= '0' && event.key <= '9') {
            event.preventDefault();
            // Two digits max — MAX_ELEMENTS is 50.
            typed = (typed + event.key).slice(0, 2);
        }
    }

    onMount(() => {
        void (async () => {
            unlistenFinal = await listen<{ text?: string; source?: string | null }>(
                'voice-final',
                (evt) => {
                    if ((evt.payload?.source ?? null) !== UI_SOURCE) return;
                    handleFinal(evt.payload?.text ?? '');
                },
            );
            unlistenReset = await listen('ui-elements-reset', () => reset());
            // #17 — a higher-priority surface (push-to-talk) took the
            // mic. The overlay can't function voice-less; just hide it
            // (its recognizer is already stopped — don't re-stop it).
            unlistenPreempted = await listen<{ client?: string }>(
                'voice-preempted',
                (evt) => {
                    if (evt.payload?.client === UI_SOURCE) void hideWindow();
                },
            );
        })();
        // Covers the first show — onMount fires once; the window is
        // reused, and `ui-elements-reset` re-arms it on later shows.
        reset();
    });

    onDestroy(() => {
        unlistenFinal?.();
        unlistenReset?.();
        unlistenPreempted?.();
        void invoke('voice_stop_continuous').catch(() => {});
    });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="ui-root" role="presentation">
    {#each elements as el, i (i)}
        <div
            class="badge"
            style:left="{el.fx * 100}%"
            style:top="{el.fy * 100}%"
        >
            <span class="badge-num">{i + 1}</span>
            {#if el.label.length > 0}
                <span class="badge-label">{el.label}</span>
            {/if}
        </div>
    {/each}

    {#if elements.length === 0}
        <div class="ui-empty" role="status">
            No clickable elements were found in the active window.
        </div>
    {/if}

    <div class="ui-hint" role="status">
        Say a <strong>number</strong> or a control's <strong>label</strong> to
        click it{#if typed}<span class="typed"> · typed {typed}</span>{/if} ·
        <strong>“cancel”</strong> to close
    </div>
</div>

<style>
    /* The window is transparent (.transparent(true) in lib.rs). Force
       the webview's own layers transparent so only the badges and the
       hint paint over whatever app is underneath. */
    :global(html),
    :global(body),
    :global(#app) {
        background: transparent !important;
        margin: 0;
        padding: 0;
        overflow: hidden;
    }

    .ui-root {
        position: fixed;
        inset: 0;
        font-family: system-ui, sans-serif;
    }

    /* A numbered badge, centered on its control. Compact so a dense
       toolbar's badges stay individually readable even when they
       crowd. */
    .badge {
        position: fixed;
        transform: translate(-50%, -50%);
        display: inline-flex;
        align-items: center;
        gap: 5px;
        max-width: 190px;
        padding: 2px 7px 2px 3px;
        border-radius: 999px;
        background: rgba(15, 15, 18, 0.93);
        box-shadow: 0 3px 12px rgba(0, 0, 0, 0.5);
        pointer-events: none;
    }

    /* The number — the always-reliable way to pick a control. */
    .badge-num {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 20px;
        height: 20px;
        padding: 0 5px;
        border-radius: 999px;
        background: #ffcf33;
        color: #1a1a1a;
        font-size: 12.5px;
        font-weight: 800;
    }

    /* The control's label — enables label-speak and tells the user what
       the badge points at. Clipped so a long label can't dominate. */
    .badge-label {
        max-width: 150px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: #f4f4f5;
        font-size: 11.5px;
        font-weight: 600;
    }

    .ui-empty {
        position: fixed;
        left: 50%;
        top: 50%;
        transform: translate(-50%, -50%);
        padding: 12px 18px;
        border-radius: 12px;
        background: rgba(15, 15, 18, 0.92);
        color: #f4f4f5;
        font-size: 13.5px;
        box-shadow: 0 6px 22px rgba(0, 0, 0, 0.45);
    }

    .ui-hint {
        position: fixed;
        left: 50%;
        bottom: 28px;
        transform: translateX(-50%);
        padding: 7px 14px;
        border-radius: 999px;
        background: rgba(15, 15, 18, 0.92);
        color: #f4f4f5;
        font-size: 12.5px;
        white-space: nowrap;
        box-shadow: 0 6px 22px rgba(0, 0, 0, 0.45);
    }
    .ui-hint strong {
        color: #ffcf33;
        font-weight: 700;
    }
    .ui-hint .typed {
        color: #ffcf33;
        font-weight: 700;
    }
</style>
