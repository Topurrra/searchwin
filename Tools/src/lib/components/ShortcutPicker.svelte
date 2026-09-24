<script lang="ts">
    /*
      Visual keyboard shortcut picker.

      Replaces a raw text input like `CommandOrControl+Alt+S` with a
      pill-style "press a key combo" capture surface.

      Critical mechanic: while the picker is in capture mode we ASK THE
      BACKEND TO TEMPORARILY UNREGISTER all global shortcuts. Without
      this step the OS routes the user's chord (e.g. their current
      Ctrl+Shift+V) to the global handler BEFORE the webview's keydown
      listener fires — so the picker would only ever see modifiers and
      the existing overlay would pop instead.

      Flow:
        - Click picker → call `suspend_global_shortcuts_for_capture`,
          enter capture mode
        - Read keydown/keyup events as the user holds + releases their
          chord
        - On release with a valid mod + non-modifier key combo: commit
          the new value (the settings store's auto-apply will re-register
          the new shortcut)
        - On Esc / outside-click cancel: call
          `resume_global_shortcuts_after_capture` to put the old shortcut
          back

      Output format matches Tauri's `Shortcut::parse` accepted syntax:
      `CommandOrControl+Shift+V` — we use `CommandOrControl` (not raw
      `Ctrl`) so the saved value is portable across OSes if we ship
      macOS / Linux later.
    */
    import { invoke } from '@tauri-apps/api/core';
    import { onMount, onDestroy } from 'svelte';
    import { Keyboard, RotateCcw } from '@lucide/svelte';
    import { _ } from 'svelte-i18n';

    let {
        value = $bindable(''),
        defaultShortcut = '',
        placeholder = undefined,
        onchange = undefined,
    }: {
        value: string;
        defaultShortcut?: string;
        placeholder?: string;
        /** Fired whenever the value is committed or reset. Callers that own
         *  their chord in a LIST (Settings → Per-app hotkeys, where each row
         *  lives inside a store array) can't use `bind:value`, so they read
         *  the new chord here instead. `bind:value` callers can ignore it. */
        onchange?: (value: string) => void;
    } = $props();

    /** Localized fallback placeholder when a caller doesn't pass one. */
    let resolvedPlaceholder = $derived(placeholder ?? $_('nav.shortcutPlaceholder'));

    let capturing = $state(false);
    let capturedMods = $state({ ctrl: false, alt: false, shift: false, meta: false });
    let capturedKey = $state('');
    let originalValue = '';

    let rootEl: HTMLDivElement | null = $state(null);

    let prettyParts = $derived(parseShortcut(value));

    function parseShortcut(raw: string): string[] {
        if (!raw) return [];
        return raw
            .split('+')
            .map((part) => part.trim())
            .filter((part) => part.length > 0)
            .map((part) => {
                if (part === 'CommandOrControl' || part === 'Control') return 'Ctrl';
                if (part === 'Meta' || part === 'Super') return 'Win';
                return part;
            });
    }

    async function startCapture() {
        if (capturing) return;
        originalValue = value;
        capturing = true;
        capturedMods = { ctrl: false, alt: false, shift: false, meta: false };
        capturedKey = '';
        // Pull existing global shortcuts off the OS-level listener so the
        // chord we're about to record isn't swallowed by one of our own
        // hotkeys firing instead. Backend is best-effort — if it fails we
        // still proceed (worst case: user's combo doesn't capture and they
        // try again or pick a different combo).
        try {
            await invoke('suspend_global_shortcuts_for_capture');
        } catch (error) {
            console.warn('shortcut suspend failed (continuing anyway):', error);
        }
        queueMicrotask(() => rootEl?.focus());
    }

    async function commitAndRestore(committedValue: string) {
        value = committedValue;
        onchange?.(committedValue);
        capturing = false;
        capturedMods = { ctrl: false, alt: false, shift: false, meta: false };
        capturedKey = '';
        // The settings store will pick up the value change via its
        // subscriber + `scheduleHotkeyApply` and call the matching
        // apply_*_hotkey_config which RE-registers the new shortcut. So
        // we don't need to manually resume here when the value changed.
        // But we DO want to make sure the OTHER shortcut (whichever this
        // picker isn't editing) is re-registered. The resume call is
        // idempotent and registers based on cached active_shortcut, so
        // it's safe to call even on commit.
        try {
            await invoke('resume_global_shortcuts_after_capture');
        } catch (error) {
            console.warn('shortcut resume failed:', error);
        }
    }

    async function cancelCapture() {
        value = originalValue;
        capturing = false;
        capturedMods = { ctrl: false, alt: false, shift: false, meta: false };
        capturedKey = '';
        try {
            await invoke('resume_global_shortcuts_after_capture');
        } catch (error) {
            console.warn('shortcut resume failed:', error);
        }
    }

    async function resetToDefault(event: MouseEvent) {
        event.stopPropagation();
        if (!defaultShortcut) return;
        // If we're already on the default, nothing to do.
        if (value === defaultShortcut) return;
        value = defaultShortcut;
        onchange?.(defaultShortcut);
        // Settings store will apply the new value through its watcher.
    }

    function keyToToken(event: KeyboardEvent): string | null {
        const key = event.key;
        if (/^[a-zA-Z]$/.test(key)) return key.toUpperCase();
        if (/^[0-9]$/.test(key)) return key;
        const named: Record<string, string> = {
            ' ': 'Space',
            Enter: 'Enter',
            Tab: 'Tab',
            Backspace: 'Backspace',
            Delete: 'Delete',
            Insert: 'Insert',
            Home: 'Home',
            End: 'End',
            PageUp: 'PageUp',
            PageDown: 'PageDown',
            ArrowUp: 'ArrowUp',
            ArrowDown: 'ArrowDown',
            ArrowLeft: 'ArrowLeft',
            ArrowRight: 'ArrowRight',
            Escape: 'Escape',
        };
        if (named[key]) return named[key];
        if (/^F([1-9]|1[0-9]|2[0-4])$/.test(key)) return key;
        if (
            event.code &&
            /^(Comma|Period|Slash|Semicolon|Quote|BracketLeft|BracketRight|Backslash|Minus|Equal|Backquote)$/.test(
                event.code,
            )
        ) {
            return event.code;
        }
        return null;
    }

    function isModifier(key: string): boolean {
        return key === 'Control' || key === 'Alt' || key === 'Shift' || key === 'Meta';
    }

    function onCaptureKeyDown(event: KeyboardEvent) {
        if (!capturing) return;
        event.preventDefault();
        event.stopPropagation();

        if (event.key === 'Escape') {
            void cancelCapture();
            return;
        }

        capturedMods = {
            ctrl: event.ctrlKey,
            alt: event.altKey,
            shift: event.shiftKey,
            meta: event.metaKey,
        };

        const token = keyToToken(event);
        if (token && !isModifier(event.key)) {
            capturedKey = token;

            // Commit immediately on the keydown — waiting until keyup
            // means a fast keypress can be missed (some keyboards
            // send the keyup before our handler reads the mod state).
            // But we still require a modifier so a bare letter doesn't
            // register a useless global hotkey.
            const hasModifier =
                capturedMods.ctrl ||
                capturedMods.alt ||
                capturedMods.shift ||
                capturedMods.meta;
            if (hasModifier) {
                const parts: string[] = [];
                if (capturedMods.ctrl || capturedMods.meta) parts.push('CommandOrControl');
                if (capturedMods.shift) parts.push('Shift');
                if (capturedMods.alt) parts.push('Alt');
                parts.push(token);
                void commitAndRestore(parts.join('+'));
            }
        }
    }

    function onCaptureKeyUp(event: KeyboardEvent) {
        if (!capturing) return;
        // We commit on keydown (above), so keyup only matters for
        // updating the modifier preview when the user releases a
        // modifier early. We don't need to do anything here for the
        // commit path.
        capturedMods = {
            ctrl: event.ctrlKey,
            alt: event.altKey,
            shift: event.shiftKey,
            meta: event.metaKey,
        };
    }

    function onDocumentKeyDown(event: KeyboardEvent) {
        if (!capturing) return;
        onCaptureKeyDown(event);
    }
    function onDocumentKeyUp(event: KeyboardEvent) {
        if (!capturing) return;
        onCaptureKeyUp(event);
    }

    /** Outside-click cancel — clicking anywhere outside the picker
     * while in capture mode bails out. */
    function onDocumentClick(event: MouseEvent) {
        if (!capturing) return;
        if (rootEl && rootEl.contains(event.target as Node)) return;
        void cancelCapture();
    }

    onMount(() => {
        document.addEventListener('keydown', onDocumentKeyDown, true);
        document.addEventListener('keyup', onDocumentKeyUp, true);
        document.addEventListener('mousedown', onDocumentClick, true);
    });
    onDestroy(() => {
        document.removeEventListener('keydown', onDocumentKeyDown, true);
        document.removeEventListener('keyup', onDocumentKeyUp, true);
        document.removeEventListener('mousedown', onDocumentClick, true);
        // If the component is destroyed mid-capture, restore shortcuts so
        // the user isn't stuck with no hotkeys.
        if (capturing) {
            void invoke('resume_global_shortcuts_after_capture').catch(() => {});
        }
    });

    let liveParts = $derived.by(() => {
        if (!capturing) return [];
        const parts: string[] = [];
        if (capturedMods.ctrl) parts.push('Ctrl');
        if (capturedMods.shift) parts.push('Shift');
        if (capturedMods.alt) parts.push('Alt');
        if (capturedMods.meta) parts.push('Win');
        if (capturedKey) parts.push(capturedKey);
        return parts;
    });

    let isOnDefault = $derived(defaultShortcut.length > 0 && value === defaultShortcut);
</script>

<div class="picker-wrap">
    <div
        class="picker"
        class:capturing
        bind:this={rootEl}
        role="button"
        tabindex="0"
        onclick={() => (capturing ? void cancelCapture() : void startCapture())}
        onkeydown={(e) => {
            if (!capturing && (e.key === 'Enter' || e.key === ' ')) {
                e.preventDefault();
                void startCapture();
            }
        }}
        aria-label={capturing ? $_('nav.shortcutPressToRecord') : $_('nav.shortcutCurrent', { values: { value: value || $_('nav.shortcutUnset') } })}
    >
        {#if capturing}
            <Keyboard class="picker-icon" />
            <span class="picker-prompt">
                {#if liveParts.length > 0}
                    {#each liveParts as part, i (i)}
                        {#if i > 0}<span class="picker-plus">+</span>{/if}
                        <kbd class="picker-kbd live">{part}</kbd>
                    {/each}
                {:else}
                    <span class="picker-hint">{$_('nav.shortcutPressPrompt')}</span>
                {/if}
            </span>
        {:else if prettyParts.length > 0}
            <span class="picker-chips">
                {#each prettyParts as part, i (i)}
                    {#if i > 0}<span class="picker-plus">+</span>{/if}
                    <kbd class="picker-kbd">{part}</kbd>
                {/each}
            </span>
        {:else}
            <Keyboard class="picker-icon" />
            <span class="picker-placeholder">{resolvedPlaceholder}</span>
        {/if}
    </div>

    <!-- Reset-to-default sits beside the picker, not inside it, so the
         picker stays a clean "single click captures" surface. Hidden when
         we have no known default OR when we're already on it. -->
    {#if defaultShortcut && !isOnDefault && !capturing}
        <button
            type="button"
            class="picker-reset"
            onclick={resetToDefault}
            title={$_('nav.shortcutResetTooltip', { values: { value: parseShortcut(defaultShortcut).join(' + ') } })}
            aria-label={$_('nav.shortcutResetAria')}
        >
            <RotateCcw class="h-3 w-3" />
        </button>
    {/if}
</div>

<style>
    .picker-wrap {
        display: inline-flex;
        align-items: center;
        gap: 6px;
    }

    .picker {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 6px 10px;
        min-height: 34px;
        min-width: 200px;
        border-radius: 10px;
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        cursor: pointer;
        transition: border-color 140ms ease, background-color 140ms ease, box-shadow 140ms ease;
    }

    .picker:hover {
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
    }

    .picker.capturing {
        border-color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 10%, var(--color-panel-2));
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 18%, transparent);
    }

    .picker:focus-visible {
        outline: none;
        box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-accent) 55%, transparent);
    }

    :global(.picker-icon) {
        width: 14px;
        height: 14px;
        color: var(--color-muted);
        flex-shrink: 0;
    }

    .picker-prompt,
    .picker-chips {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        flex: 1;
    }

    .picker-hint {
        font-size: 12px;
        color: var(--color-text-secondary);
        font-style: italic;
    }

    .picker-placeholder {
        font-size: 12px;
        color: var(--color-muted);
    }

    .picker-plus {
        color: var(--color-muted);
        font-size: 11px;
    }

    .picker-kbd {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 28px;
        height: 24px;
        padding: 0 7px;
        border: 1px solid var(--color-border);
        border-radius: 6px;
        background: var(--color-panel);
        color: var(--color-text);
        font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
        font-size: 11.5px;
        font-weight: 500;
        line-height: 1;
        box-shadow: 0 1px 0 rgba(0, 0, 0, 0.25);
    }

    .picker-kbd.live {
        border-color: color-mix(in srgb, var(--color-accent) 50%, var(--color-border));
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 12%, var(--color-panel));
    }

    .picker-reset {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 26px;
        height: 26px;
        border-radius: 7px;
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
        cursor: pointer;
        transition: background-color 120ms ease, color 120ms ease, border-color 120ms ease;
    }

    .picker-reset:hover {
        background: var(--color-panel);
        border-color: color-mix(in srgb, var(--color-accent) 50%, var(--color-border));
        color: var(--color-text);
    }
</style>
