/**
 * Quality Pass Wave 1 / Esc-Sweep (2026-05-29): tiny Svelte action that
 * wires "press Esc to clear this input" everywhere the pattern makes
 * sense — search boxes, filter inputs, template fields, etc.
 *
 * The point is muscle memory. Power users (the ones we're freeing from
 * cloud subscriptions) expect that hitting Esc in any text field of any
 * good app empties it without a second click. Apple Spotlight, VS Code
 * Command Palette, Windows search, every IDE — they all do this. We
 * make sure every single search / filter / quick-template input in
 * KeepItLocal does too.
 *
 * Safety net: when the input is already empty we DO NOT swallow the
 * key event. The Esc bubbles up so dialogs / overlays / the command
 * palette can use it for "close me". Only an input with text content
 * intercepts.
 *
 * Usage (Svelte 5 runes):
 *   let query = $state('');
 *   <input bind:value={query} use:escToClear={() => (query = '')} />
 *
 * Or with a writable store value:
 *   <input bind:value={$store} use:escToClear={() => store.set('')} />
 *
 * @param node   The input / textarea the action is bound to.
 * @param clear  Caller-supplied "empty the bound value" function. The
 *               action calls this when Esc is pressed on a non-empty
 *               input.
 */
export function escToClear(
    node: HTMLInputElement | HTMLTextAreaElement,
    clear: () => void,
) {
    let current = clear;

    // Typed as the generic `EventListener` so addEventListener accepts it
    // without TS struggling to pick the right overload — we narrow back
    // to KeyboardEvent inside.
    const handler: EventListener = (event) => {
        const e = event as KeyboardEvent;
        if (e.key !== 'Escape') return;
        // Empty input → don't intercept. Let the event bubble so
        // overlays, palettes and dialogs can close on Esc as usual.
        if (node.value.length === 0) return;
        // Non-empty → claim the event, clear, and prevent any other
        // listener on this input (e.g. a tool's own `onkeydown` that
        // also reacts to Escape) from firing. stopImmediatePropagation
        // is the one that locks out same-target listeners; the regular
        // stopPropagation only prevents the event from bubbling.
        e.preventDefault();
        e.stopPropagation();
        e.stopImmediatePropagation();
        current();
        // Some host components debounce on the next input event;
        // dispatch one so they observe the cleared value immediately.
        node.dispatchEvent(new Event('input', { bubbles: true }));
    };

    // Capture phase = our handler fires BEFORE any bubble-phase
    // keydown handler that was registered earlier on the same input
    // (Svelte's inline `onkeydown={...}` is bubble-phase). Combined
    // with stopImmediatePropagation above, we win the Escape race
    // cleanly without forcing every caller to remove its existing
    // Escape handler.
    node.addEventListener('keydown', handler, { capture: true });

    return {
        /** Lets the caller swap the clear callback at runtime (e.g. when
         *  the bound store identity changes after route navigation). */
        update(next: () => void) {
            current = next;
        },
        destroy() {
            node.removeEventListener('keydown', handler, { capture: true });
        },
    };
}
