<!--
  PrivacyBlur — Phase 6.5-3 (2026-05-27).

  Renders a string with sensitive-content spans visually blurred via CSS
  `filter: blur(…)`. Click a blurred span to toggle reveal; click again
  to re-blur. Ctrl+Shift+R reveals every span at once while the
  container has focus. The unblurred text is always available via the
  optional Copy affordance — copying never reveals visually.

  Layout-stable: CSS filter blur doesn't change box dimensions, so
  surrounding text never reflows when a span is toggled. The blurred
  span is `user-select: none` so a stray drag-select doesn't leak the
  underlying glyphs into the OS clipboard.

  Findings come from the backend `scan_text_for_findings` command. The
  primitive doesn't run any detection itself — it's purely a renderer.

  Threat model recap (see DESIGN.md):
    • Shoulder-surfing — the primary scenario this defends against.
      A bystander glancing at the screen sees "[blurred]", not the
      secret. The user clicks to reveal when they actually need it.
    • Accidental screen sharing — the secret stays blurred until the
      user explicitly reveals it. Screenshare software captures the
      blurred pixels.
    • Local attacker with full screen access — NOT defended (they can
      just open the source). Not the goal.

  Used by:
    - SecretLeakScanner.svelte (Wave 6.5-4)
    - Clipboard history previews (Wave 6.5-4)
    - Privacy Audit Unencrypted Secrets (Wave 6.5-4)
-->
<script lang="ts">
    import { onMount, onDestroy } from 'svelte';
    import { SvelteSet } from 'svelte/reactivity';
    import { Eye, EyeOff, Copy } from '@lucide/svelte';
    import type { SensitiveFinding, SensitiveTier } from '$lib/types/sensitive';

    interface Props {
        /** The full source string. Slicing by finding spans recovers
         *  each matched substring. */
        text: string;
        /** Sensitive-content findings from `scan_text_for_findings`.
         *  Spans MUST be valid byte ranges within `text`; out-of-bounds
         *  findings are skipped defensively. */
        findings?: SensitiveFinding[];
        /** Which tiers to actually blur. Default ['high'] — matches the
         *  Phase 6.5 policy (medium surfaces in tools, low never blurs
         *  by default). Pass ['high','medium'] for stricter contexts
         *  like the Secret Leak Scanner's Security preset, or
         *  ['high','medium','low'] to blur everything (Release check). */
        blurTiers?: SensitiveTier[];
        /** Show a small Copy-all button in the bottom-right corner.
         *  Copies the unblurred source text WITHOUT visually revealing
         *  any span — paste-share is intentional. */
        showCopy?: boolean;
        /** CSS blur radius in px. Default 6 (legible "something's here"
         *  shape; not so soft the user can't aim the click). */
        blurPx?: number;
        /** Optional callback fired after the copy succeeds. Lets the
         *  consumer show a toast or update state. */
        oncopy?: (copied: string) => void;
    }

    let {
        text,
        findings = [],
        blurTiers = ['high'],
        showCopy = false,
        blurPx = 6,
        oncopy,
    }: Props = $props();

    /** Per-span reveal state, keyed by the position-in-render index.
     *  SvelteSet gives fine-grained reactivity so toggling one span
     *  doesn't invalidate the others. */
    const revealed = new SvelteSet<number>();

    /** Container DOM ref so the global Ctrl+Shift+R handler can check
     *  whether focus is inside us before claiming the shortcut. */
    let containerEl: HTMLDivElement | null = $state(null);

    interface PlainSegment {
        type: 'plain';
        text: string;
    }
    interface BlurSegment {
        type: 'blur';
        text: string;
        finding: SensitiveFinding;
        /** Stable index across re-renders so reveal state survives a
         *  $derived recompute. */
        index: number;
    }
    type Segment = PlainSegment | BlurSegment;

    /** Build the alternating plain/blur segment list. Findings can
     *  overlap (e.g. a long JWT contains a shorter token match) —
     *  we sort start asc, end DESC (longer wins on tie), then walk
     *  forward, skipping anything already covered. */
    const segments: Segment[] = $derived.by(() => {
        if (!text) return [];
        const blurredSet = new Set(blurTiers);
        // Filter to tiers we actually blur + bounds-check.
        const eligible = findings.filter(
            (f) =>
                blurredSet.has(f.tier) &&
                f.start >= 0 &&
                f.end <= text.length &&
                f.start < f.end,
        );
        const sorted = [...eligible].sort(
            (a, b) => a.start - b.start || b.end - a.end,
        );

        const out: Segment[] = [];
        let cursor = 0;
        let blurIndex = 0;
        for (const f of sorted) {
            if (f.start < cursor) continue; // overlapped by a longer earlier span
            if (f.start > cursor) {
                out.push({ type: 'plain', text: text.slice(cursor, f.start) });
            }
            out.push({
                type: 'blur',
                text: text.slice(f.start, f.end),
                finding: f,
                index: blurIndex++,
            });
            cursor = f.end;
        }
        if (cursor < text.length) {
            out.push({ type: 'plain', text: text.slice(cursor) });
        }
        return out;
    });

    /** Number of blur spans, drives the "Reveal all" button visibility +
     *  the screen-reader summary. */
    const blurCount = $derived(
        segments.reduce((acc, s) => acc + (s.type === 'blur' ? 1 : 0), 0),
    );

    function toggle(index: number) {
        if (revealed.has(index)) {
            revealed.delete(index);
        } else {
            revealed.add(index);
        }
    }

    /** Reveal every blurred span in the container. Invoked by the
     *  toolbar button + Ctrl+Shift+R (when focus is inside). */
    function revealAll() {
        for (const seg of segments) {
            if (seg.type === 'blur') revealed.add(seg.index);
        }
    }

    /** Re-blur every revealed span. Mirror of revealAll. */
    function blurAll() {
        revealed.clear();
    }

    async function copyAll() {
        try {
            await navigator.clipboard.writeText(text);
            oncopy?.(text);
        } catch {
            // Clipboard API can fail (e.g. document not focused) — silent
            // is fine; the consumer's onCopy will see no callback.
        }
    }

    /** Global keydown — Ctrl+Shift+R while focus is inside the
     *  container reveals every blurred span. We listen on the document
     *  so the shortcut works whether the user has focused a specific
     *  span (button) or the container itself. Cleaned up on destroy. */
    function onGlobalKeydown(event: KeyboardEvent) {
        if (!event.ctrlKey || !event.shiftKey) return;
        // Use KeyR (physical key) — locale-independent.
        if (event.code !== 'KeyR') return;
        if (!containerEl) return;
        if (!containerEl.contains(document.activeElement)) return;
        event.preventDefault();
        if (revealed.size === blurCount) {
            blurAll();
        } else {
            revealAll();
        }
    }

    onMount(() => {
        document.addEventListener('keydown', onGlobalKeydown);
    });
    onDestroy(() => {
        document.removeEventListener('keydown', onGlobalKeydown);
    });
</script>

<div
    class="pb"
    bind:this={containerEl}
    style="--pb-blur: {blurPx}px"
    data-blur-count={blurCount}
>
    {#if blurCount > 0}
        <div class="pb-toolbar" role="toolbar" aria-label="Sensitive content controls">
            <span class="pb-toolbar-count">
                {blurCount} hidden item{blurCount === 1 ? '' : 's'}
            </span>
            <span class="pb-toolbar-spacer"></span>
            {#if revealed.size === blurCount}
                <button
                    type="button"
                    class="pb-btn"
                    onclick={blurAll}
                    title="Hide all (Ctrl+Shift+R)"
                >
                    <EyeOff class="pb-btn-ico" />
                    Hide all
                </button>
            {:else}
                <button
                    type="button"
                    class="pb-btn"
                    onclick={revealAll}
                    title="Show all (Ctrl+Shift+R)"
                >
                    <Eye class="pb-btn-ico" />
                    Reveal all
                </button>
            {/if}
            {#if showCopy}
                <button
                    type="button"
                    class="pb-btn"
                    onclick={() => void copyAll()}
                    title="Copy without revealing"
                >
                    <Copy class="pb-btn-ico" />
                    Copy
                </button>
            {/if}
        </div>
    {/if}

    <div class="pb-body">
        {#each segments as seg, i (i)}
            {#if seg.type === 'plain'}<span class="pb-plain">{seg.text}</span>{:else}
                {@const isRevealed = revealed.has(seg.index)}
                <button
                    type="button"
                    class="pb-span"
                    class:is-revealed={isRevealed}
                    onclick={() => toggle(seg.index)}
                    aria-label={isRevealed
                        ? `${seg.finding.kind} revealed, click to hide`
                        : `${seg.finding.kind} hidden, click to reveal`}
                    aria-pressed={isRevealed}
                    data-kind={seg.finding.kind}
                    data-tier={seg.finding.tier}
                    >{seg.text}</button>
            {/if}
        {/each}
    </div>
</div>

<style>
    .pb {
        display: block;
        font-family: inherit;
    }

    /* Optional toolbar — only renders when there's at least one
       blurred span. Keeps the primitive's footprint near zero when
       the input has no findings. */
    .pb-toolbar {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 6px 8px;
        margin-bottom: 6px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 8px;
        font-size: 11.5px;
    }
    .pb-toolbar-count {
        color: var(--color-text-secondary);
        font-variant-numeric: tabular-nums;
    }
    .pb-toolbar-spacer {
        flex: 1;
    }
    .pb-btn {
        appearance: none;
        border: 1px solid var(--color-border);
        background: transparent;
        color: var(--color-text);
        padding: 3px 8px;
        font-size: 11.5px;
        border-radius: 6px;
        cursor: pointer;
        display: inline-flex;
        align-items: center;
        gap: 5px;
        transition: background 120ms ease, border-color 120ms ease;
    }
    .pb-btn:hover {
        background: var(--color-panel-3);
        border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
    }
    .pb-btn :global(.pb-btn-ico) {
        width: 13px;
        height: 13px;
    }

    .pb-body {
        white-space: pre-wrap;
        word-break: break-word;
        line-height: 1.55;
    }
    .pb-plain {
        white-space: pre-wrap;
    }

    /* The actual blurred span. button reset so it lives inline like
       text but is keyboard-activatable (Enter / Space). Blur is the
       primary effect; the rest preserves text geometry. */
    .pb-span {
        appearance: none;
        border: 0;
        padding: 0 2px;
        margin: 0;
        background: color-mix(in srgb, var(--color-accent) 18%, transparent);
        color: inherit;
        font: inherit;
        cursor: pointer;
        border-radius: 3px;
        /* The whole point: hide the glyphs from a bystander's eye while
           the layout stays identical (CSS filter doesn't change box
           dimensions). 120ms transition is short enough to feel
           responsive but smooth, matching the rest of the app's
           motion language. */
        filter: blur(var(--pb-blur));
        /* Block accidental drag-select while blurred — otherwise the
           OS clipboard would receive the unblurred underlying text. */
        user-select: none;
        -webkit-user-select: none;
        transition: filter 120ms ease, background 120ms ease;
    }
    .pb-span:hover {
        background: color-mix(in srgb, var(--color-accent) 28%, transparent);
    }
    .pb-span:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: 2px;
    }
    .pb-span.is-revealed {
        filter: none;
        user-select: text;
        -webkit-user-select: text;
        background: color-mix(in srgb, var(--color-accent) 22%, transparent);
    }
</style>
