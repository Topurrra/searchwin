<script lang="ts">
    /*
      Per-entry quick-action menu for clipboard history.

      Click the wand icon on a clipboard entry → a small popover lists
      transformations relevant to the entry's category (Format JSON for
      JSON entries, Decode for base64-looking, etc.) plus a "More…"
      section with universal transforms (uppercase, sort, etc.).

      The action runs through the backend's `run_clipboard_action`
      command (pure text-in/text-out), and the result is copied back
      to the clipboard with a toast. The original entry stays put —
      we don't replace it, because the user might want both versions
      in their history.
    */
    import { invoke } from '@tauri-apps/api/core';
    import { onMount, onDestroy } from 'svelte';
    import {
        Wand2,
        Braces,
        Binary,
        Link as LinkIcon,
        CaseSensitive,
        ArrowDown01,
        Layers,
        ArrowDownUp,
        ChevronRight,
        NotebookPen,
        QrCode,
    } from '@lucide/svelte';
    import { toast } from '$lib/stores/toasts';
    import { captureNote } from '$lib/stores/notes';
    import { _ } from 'svelte-i18n';
    import { t } from '$lib/i18n';

    let {
        text,
        category = 'text',
    }: {
        text: string;
        category?: string;
    } = $props();

    let open = $state(false);
    let qrPreview = $state<string | null>(null);
    let qrGenerating = $state(false);
    let containerEl: HTMLDivElement | null = $state(null);
    let triggerEl: HTMLButtonElement | null = $state(null);
    /** Bound to the portaled popover so the outside-click handler can
     *  distinguish "click on the popover itself or its descendants"
     *  (don't close) from "click anywhere else in the document"
     *  (close). Required because the popover is portaled to body —
     *  it's no longer a descendant of `containerEl`. */
    let popoverEl: HTMLDivElement | null = $state(null);
    /** Cleanup Wave 1.1 (2026-05-28): the popover used to live inside
     *  the same flex column as the row actions, with `position: absolute`.
     *  Inside `.ch-rows-scroll` (overflow-y: auto, max-height: 60vh) the
     *  bottom of the popover got clipped — the user reported "modal is
     *  not fully visible". Switched to position: fixed + computed
     *  coordinates from the trigger's bounding rect so the popover
     *  escapes the scroll container entirely. Also flips above the
     *  trigger when there isn't enough room below.
     *
     *  Cleanup Wave 1.2 (2026-05-28): position: fixed alone wasn't enough
     *  inside the command palette — `.cmd-panel` has `backdrop-filter`
     *  which creates a containing block for fixed descendants AND
     *  `overflow: hidden` which clips them. The popover existed in the
     *  DOM but rendered invisible. Fix: portal the popover element to
     *  `document.body` via a use:portal action so it escapes every
     *  ancestor containing block + clip. Body itself only has clip-path
     *  to the rounded window outline; a fixed popover inside body is
     *  positioned relative to the viewport and visible. */
    let popoverTop = $state(0);
    let popoverLeft = $state(0);
    let popoverFlipAbove = $state(false);
    const POPOVER_MAX_HEIGHT = 380;
    const POPOVER_WIDTH = 320;
    const POPOVER_GAP = 6;

    /** Svelte action that re-parents its node to `document.body` on mount
     *  and removes it on destroy. Lets the popover escape ancestors that
     *  would otherwise clip it (`overflow: hidden`) or break its fixed
     *  positioning (any `transform` / `filter` / `backdrop-filter` /
     *  `will-change` ancestor creates a new containing block for fixed
     *  descendants, so `top`/`left` stop being viewport-relative). */
    function portal(node: HTMLElement) {
        document.body.appendChild(node);
        return {
            destroy() {
                if (node.parentNode === document.body) {
                    document.body.removeChild(node);
                }
            },
        };
    }

    type ActionKind =
        | 'format_json'
        | 'minify_json'
        | 'decode_base64'
        | 'encode_base64'
        | 'url_decode'
        | 'url_encode'
        | 'uppercase'
        | 'lowercase'
        | 'title_case'
        | 'trim'
        | 'line_sort'
        | 'line_dedupe'
        | 'line_reverse'
        | 'reverse_string';

    // `label`/`description` hold i18n KEY strings (resolved with `$_`/`t`
    // at render time) rather than literal English, so the action menu
    // translates with the locale.
    type ActionItem = {
        kind: ActionKind;
        label: string;
        description: string;
        icon: typeof Braces;
    };

    /** Default set of "always relevant" universal transforms. Shown
     * after the category-specific recommendations. */
    const UNIVERSAL: ActionItem[] = [
        { kind: 'uppercase', label: 'nav.clip.uppercase', description: 'nav.clip.uppercaseDesc', icon: CaseSensitive },
        { kind: 'lowercase', label: 'nav.clip.lowercase', description: 'nav.clip.lowercaseDesc', icon: CaseSensitive },
        { kind: 'title_case', label: 'nav.clip.titleCase', description: 'nav.clip.titleCaseDesc', icon: CaseSensitive },
        { kind: 'trim', label: 'nav.clip.trim', description: 'nav.clip.trimDesc', icon: ArrowDown01 },
        { kind: 'line_sort', label: 'nav.clip.lineSort', description: 'nav.clip.lineSortDesc', icon: ArrowDownUp },
        { kind: 'line_dedupe', label: 'nav.clip.lineDedupe', description: 'nav.clip.lineDedupeDesc', icon: Layers },
        { kind: 'line_reverse', label: 'nav.clip.lineReverse', description: 'nav.clip.lineReverseDesc', icon: ArrowDownUp },
        { kind: 'reverse_string', label: 'nav.clip.reverseString', description: 'nav.clip.reverseStringDesc', icon: ArrowDownUp },
    ];

    /** Category → recommended actions. Empty for categories where no
     * transform is obviously the right one. */
    let recommended = $derived.by((): ActionItem[] => {
        switch (category) {
            case 'json':
                return [
                    { kind: 'format_json', label: 'nav.clip.formatJson', description: 'nav.clip.formatJsonDesc', icon: Braces },
                    { kind: 'minify_json', label: 'nav.clip.minifyJson', description: 'nav.clip.minifyJsonDesc', icon: Braces },
                ];
            case 'url':
                return [
                    { kind: 'url_decode', label: 'nav.clip.urlDecode', description: 'nav.clip.urlDecodeDescArrow', icon: LinkIcon },
                    { kind: 'url_encode', label: 'nav.clip.urlEncode', description: 'nav.clip.urlEncodeDescArrow', icon: LinkIcon },
                ];
            case 'code':
            case 'text':
                // Best guess for base64: any text that's all the right characters
                // and length divisible by 4. Cheap heuristic; the user can still
                // try Decode on anything from the universal menu.
                if (looksLikeBase64(text)) {
                    return [
                        { kind: 'decode_base64', label: 'nav.clip.decodeBase64', description: 'nav.clip.decodeBase64DescGuess', icon: Binary },
                    ];
                }
                return [];
            default:
                return [];
        }
    });

    /** Always-available encode/decode shortcuts shown after the
     * category-specific block. Surfacing these lets users run "encode
     * base64" on anything without hunting for a separate tool. */
    const TRANSFORMS: ActionItem[] = [
        { kind: 'encode_base64', label: 'nav.clip.encodeBase64', description: 'nav.clip.encodeBase64Desc', icon: Binary },
        { kind: 'decode_base64', label: 'nav.clip.decodeBase64', description: 'nav.clip.decodeBase64Desc', icon: Binary },
        { kind: 'url_encode', label: 'nav.clip.urlEncode', description: 'nav.clip.urlEncodeDesc', icon: LinkIcon },
        { kind: 'url_decode', label: 'nav.clip.urlDecode', description: 'nav.clip.urlDecodeDesc', icon: LinkIcon },
        { kind: 'format_json', label: 'nav.clip.formatJson', description: 'nav.clip.formatJsonDescValid', icon: Braces },
    ];

    /** Heuristic: looks-like-base64 if length ≥ 16, all valid base64
     * chars, length mod 4 = 0. Not perfect — strings like "test" pass
     * — so we only use it as a sort hint, not a hard gate. */
    function looksLikeBase64(value: string): boolean {
        const trimmed = value.trim();
        if (trimmed.length < 16) return false;
        if (trimmed.length % 4 !== 0) return false;
        return /^[A-Za-z0-9+/]+={0,2}$/.test(trimmed);
    }

    async function run(action: ActionItem) {
        try {
            const result = await invoke<string>('run_clipboard_action', {
                action: action.kind,
                input: text,
            });
            // Copy result to system clipboard so user can paste it
            // wherever they need. We deliberately don't replace the
            // history entry — that's destructive and not what they
            // expect from "transform this".
            await navigator.clipboard.writeText(result);
            toast(t('nav.clip.actionCopied', { actionLabel: t(action.label) }), 'success', 3500);
            open = false;
        } catch (error) {
            toast(t('nav.clip.actionFailed', { actionLabel: t(action.label), error: String(error) }), 'error', 4500);
        }
    }

    /** Save this clipboard entry as a new local note (.ki). Unlike the
     *  transforms above, this writes a file rather than copying text back —
     *  the integration that makes Clipboard feed Notes. */
    async function sendToNote() {
        // TODO(i18n): localize once the Notes l10n pass lands.
        const summary = await captureNote(text);
        if (summary) {
            toast(`Saved to note "${summary.title}"`, 'success', 3500);
        } else {
            toast('Could not save note', 'error', 4000);
        }
        open = false;
    }

    async function generateQr() {
        qrGenerating = true;
        try {
            const result = await invoke<{ png_base64: string }>('generate_qr', {
                text,
                errorCorrection: 'M',
                scale: 6,
                margin: 2,
            });
            qrPreview = `data:image/png;base64,${result.png_base64}`;
        } catch (error) {
            toast(`Could not generate QR code: ${String(error)}`, 'error', 4000);
        } finally {
            qrGenerating = false;
        }
    }

    /** Re-anchor the popover to the trigger's current screen position.
     *  Flips above the trigger if there's not enough room below.
     *  Called on open and on window scroll/resize while open. */
    function positionPopover() {
        if (!triggerEl) return;
        const rect = triggerEl.getBoundingClientRect();
        const vh = window.innerHeight;
        const vw = window.innerWidth;
        const spaceBelow = vh - rect.bottom - POPOVER_GAP;
        const spaceAbove = rect.top - POPOVER_GAP;
        popoverFlipAbove = spaceBelow < Math.min(POPOVER_MAX_HEIGHT, 240) && spaceAbove > spaceBelow;
        popoverTop = popoverFlipAbove
            ? Math.max(8, rect.top - POPOVER_GAP - Math.min(POPOVER_MAX_HEIGHT, spaceAbove))
            : rect.bottom + POPOVER_GAP;
        // Right-align with the trigger, clamped to the viewport so a
        // narrow window doesn't push the popover off-screen.
        popoverLeft = Math.min(
            Math.max(8, rect.right - POPOVER_WIDTH),
            vw - POPOVER_WIDTH - 8,
        );
    }

    function toggle(event: MouseEvent) {
        event.stopPropagation();
        if (!open) positionPopover();
        open = !open;
    }

    function onWindowReanchor() {
        if (open) positionPopover();
    }

    /** Close on outside click. Mounted at document level so any click
     * outside the menu container dismisses. Cleanup Wave 1.2: also
     * checks the portaled popover element since it's no longer a
     * descendant of containerEl. */
    function onDocumentClick(event: MouseEvent) {
        if (!open) return;
        const target = event.target as Node;
        if (containerEl && containerEl.contains(target)) return;
        if (popoverEl && popoverEl.contains(target)) return;
        open = false;
    }

    function onDocumentKey(event: KeyboardEvent) {
        if (event.key === 'Escape' && open) {
            event.preventDefault();
            open = false;
        }
    }

    onMount(() => {
        document.addEventListener('click', onDocumentClick);
        document.addEventListener('keydown', onDocumentKey);
        // Reposition the fixed-position popover when the viewport
        // changes — scroll inside .ch-rows-scroll, window resize, etc.
        // Passive + capture-phase scroll so we catch scrolls in
        // ancestor scrollers, not just the page.
        window.addEventListener('scroll', onWindowReanchor, { passive: true, capture: true });
        window.addEventListener('resize', onWindowReanchor);
    });
    onDestroy(() => {
        document.removeEventListener('click', onDocumentClick);
        document.removeEventListener('keydown', onDocumentKey);
        window.removeEventListener('scroll', onWindowReanchor, { capture: true });
        window.removeEventListener('resize', onWindowReanchor);
    });
</script>

<div class="action-menu-container" bind:this={containerEl}>
    <button
        bind:this={triggerEl}
        type="button"
        class="action-trigger"
        class:open
        onclick={toggle}
        title={$_('nav.clip.transformTooltip')}
        aria-haspopup="menu"
        aria-expanded={open}
    >
        <Wand2 class="h-3.5 w-3.5" />
    </button>

    {#if open}
        <div
            bind:this={popoverEl}
            class="action-popover"
            class:is-above={popoverFlipAbove}
            style="top: {popoverTop}px; left: {popoverLeft}px;"
            role="menu"
            tabindex="-1"
            use:portal
        >
            <div class="action-section-label">Save</div>
            <button class="action-item recommended" type="button" onclick={() => void sendToNote()}>
                <NotebookPen class="action-item-icon" />
                <div class="action-item-text">
                    <div class="action-item-label">Send to note</div>
                    <div class="action-item-desc">Save this as a new note in your notebook</div>
                </div>
                <ChevronRight class="action-item-chevron" />
            </button>
            <button class="action-item recommended" type="button" onclick={() => void generateQr()}>
                <QrCode class="action-item-icon" />
                <div class="action-item-text">
                    <div class="action-item-label">Generate QR code</div>
                    <div class="action-item-desc">Show this copied text as a QR code</div>
                </div>
                <ChevronRight class="action-item-chevron" />
            </button>
            {#if qrGenerating}
                <div class="action-qr-status" role="status">Generating QR code…</div>
            {:else if qrPreview}
                <div class="action-qr-preview" role="status">
                    <img src={qrPreview} alt="QR code for copied text" />
                    <span>QR code ready to scan</span>
                </div>
            {/if}
            <div class="action-divider"></div>

            {#if recommended.length > 0}
                <div class="action-section-label">{$_('nav.clip.sectionSuggested')}</div>
                {#each recommended as item}
                    {@const Icon = item.icon}
                    <button class="action-item recommended" type="button" onclick={() => void run(item)}>
                        <Icon class="action-item-icon" />
                        <div class="action-item-text">
                            <div class="action-item-label">{$_(item.label)}</div>
                            <div class="action-item-desc">{$_(item.description)}</div>
                        </div>
                        <ChevronRight class="action-item-chevron" />
                    </button>
                {/each}
                <div class="action-divider"></div>
            {/if}

            <div class="action-section-label">{$_('nav.clip.sectionTransform')}</div>
            {#each TRANSFORMS as item}
                {@const Icon = item.icon}
                <button class="action-item" type="button" onclick={() => void run(item)}>
                    <Icon class="action-item-icon" />
                    <div class="action-item-text">
                        <div class="action-item-label">{$_(item.label)}</div>
                        <div class="action-item-desc">{$_(item.description)}</div>
                    </div>
                </button>
            {/each}

            <div class="action-divider"></div>
            <div class="action-section-label">{$_('nav.clip.sectionUniversal')}</div>
            {#each UNIVERSAL as item}
                {@const Icon = item.icon}
                <button class="action-item" type="button" onclick={() => void run(item)}>
                    <Icon class="action-item-icon" />
                    <div class="action-item-text">
                        <div class="action-item-label">{$_(item.label)}</div>
                        <div class="action-item-desc">{$_(item.description)}</div>
                    </div>
                </button>
            {/each}
        </div>
    {/if}
</div>

<style>
    .action-menu-container {
        position: relative;
        display: inline-block;
    }

    .action-trigger {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 30px;
        height: 30px;
        border-radius: 8px;
        border: 1px solid var(--color-border);
        background: var(--color-panel);
        color: var(--color-text-secondary);
        cursor: pointer;
        transition: border-color 140ms ease, color 140ms ease, background-color 140ms ease;
    }

    .action-trigger:hover,
    .action-trigger.open {
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        color: var(--color-accent);
        background: var(--color-panel-2);
    }

    .action-popover {
        /* Cleanup Wave 1.1 (2026-05-28): position: fixed so the popover
           escapes the scroll container that was clipping it. Top/left
           are computed in JS from the trigger's bounding rect; this
           rule just sets the box style + size + animation. */
        position: fixed;
        z-index: 60;
        min-width: 280px;
        max-width: 320px;
        width: 320px;
        max-height: 380px;
        overflow-y: auto;
        padding: 6px;
        border-radius: 12px;
        border: 1px solid var(--color-border);
        background: var(--color-panel);
        box-shadow: var(--shadow-lg, 0 12px 28px rgba(0, 0, 0, 0.45));
        animation: action-popover-in 140ms ease-out;
    }

    /* Flip-above variant: same animation but slides in from BELOW the
       final position instead of above, so the entry direction matches
       the visual relationship with the trigger. */
    .action-popover.is-above {
        animation-name: action-popover-in-above;
    }
    @keyframes action-popover-in-above {
        from {
            opacity: 0;
            transform: translateY(4px);
        }
        to {
            opacity: 1;
            transform: translateY(0);
        }
    }

    @keyframes action-popover-in {
        from {
            opacity: 0;
            transform: translateY(-4px);
        }
        to {
            opacity: 1;
            transform: translateY(0);
        }
    }

    .action-section-label {
        padding: 6px 8px 4px;
        font-size: 10px;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        font-weight: 600;
        color: var(--color-muted);
    }

    .action-divider {
        height: 1px;
        background: var(--color-border);
        margin: 4px 6px;
    }

    .action-qr-status,
    .action-qr-preview {
        margin: 4px 6px 6px;
        border: 1px solid var(--color-border);
        border-radius: 8px;
        background: var(--color-panel-2);
        color: var(--color-muted);
        font-size: 10.5px;
        line-height: 1.3;
    }

    .action-qr-status {
        padding: 8px 10px;
    }

    .action-qr-preview {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 8px;
    }

    .action-qr-preview img {
        width: 72px;
        height: 72px;
        flex: 0 0 auto;
        border-radius: 4px;
        image-rendering: pixelated;
    }

    .action-item {
        display: flex;
        align-items: center;
        gap: 10px;
        width: 100%;
        padding: 8px 10px;
        border-radius: 8px;
        border: none;
        background: transparent;
        color: inherit;
        text-align: left;
        cursor: pointer;
        transition: background-color 100ms ease;
    }

    .action-item:hover {
        background: var(--color-panel-2);
    }

    .action-item.recommended {
        background: color-mix(in srgb, var(--color-accent) 6%, transparent);
    }

    .action-item.recommended:hover {
        background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    }

    :global(.action-item-icon) {
        width: 14px;
        height: 14px;
        color: var(--color-text-secondary);
        flex-shrink: 0;
    }

    .action-item.recommended :global(.action-item-icon) {
        color: var(--color-accent);
    }

    .action-item-text {
        flex: 1;
        min-width: 0;
    }

    .action-item-label {
        font-size: 12.5px;
        font-weight: 500;
        color: var(--color-text);
        line-height: 1.3;
    }

    .action-item-desc {
        font-size: 10.5px;
        line-height: 1.3;
        color: var(--color-muted);
        margin-top: 1px;
    }

    :global(.action-item-chevron) {
        width: 12px;
        height: 12px;
        color: var(--color-muted);
        flex-shrink: 0;
    }
</style>
