<script lang="ts">
    /*
      Color Picker — pick a color from anywhere on screen (Chromium EyeDropper
      API, available in WebView2) or choose one manually, then copy it in any
      common format. Fully on-device: no backend, no network.
    */
    import { Pipette, Copy, Check } from '@lucide/svelte';
    import { ToolPage, Button } from '$lib/ui';
    import { toast } from '$lib/stores/toasts';

    // The screen eyedropper is a standard Chromium API but isn't in the TS DOM
    // lib yet — feature-detect it without pulling in `any`.
    type EyeDropperResult = { sRGBHex: string };
    type EyeDropperCtor = new () => { open: (opts?: { signal?: AbortSignal }) => Promise<EyeDropperResult> };
    const EyeDropperImpl = (globalThis as unknown as { EyeDropper?: EyeDropperCtor }).EyeDropper;
    const hasEyedropper = !!EyeDropperImpl;

    const RECENT_KEY = 'keepitlocal.colorPicker.recent';
    const RECENT_MAX = 14;

    let hex = $state('#3B82F6');
    let copied = $state<string | null>(null);
    let recent = $state<string[]>(loadRecent());

    function loadRecent(): string[] {
        if (typeof localStorage === 'undefined') return [];
        try {
            const raw = localStorage.getItem(RECENT_KEY);
            const parsed = raw ? JSON.parse(raw) : [];
            return Array.isArray(parsed) ? parsed.filter((v): v is string => typeof v === 'string') : [];
        } catch {
            return [];
        }
    }

    function rememberColor(value: string) {
        const v = value.toUpperCase();
        recent = [v, ...recent.filter((c) => c.toUpperCase() !== v)].slice(0, RECENT_MAX);
        if (typeof localStorage !== 'undefined') {
            localStorage.setItem(RECENT_KEY, JSON.stringify(recent));
        }
    }

    // ── Conversions ───────────────────────────────────────────────────────
    function normalizeHex(input: string): string | null {
        let s = input.trim().replace(/^#/, '');
        if (/^[0-9a-f]{3}$/i.test(s)) {
            s = s.split('').map((c) => c + c).join('');
        }
        return /^[0-9a-f]{6}$/i.test(s) ? '#' + s.toUpperCase() : null;
    }

    const rgb = $derived.by(() => {
        const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
        if (!m) return { r: 0, g: 0, b: 0 };
        return { r: parseInt(m[1], 16), g: parseInt(m[2], 16), b: parseInt(m[3], 16) };
    });

    const hsl = $derived.by(() => {
        const r = rgb.r / 255, g = rgb.g / 255, b = rgb.b / 255;
        const max = Math.max(r, g, b), min = Math.min(r, g, b);
        const d = max - min;
        let h = 0;
        if (d !== 0) {
            if (max === r) h = ((g - b) / d) % 6;
            else if (max === g) h = (b - r) / d + 2;
            else h = (r - g) / d + 4;
            h = Math.round(h * 60);
            if (h < 0) h += 360;
        }
        const l = (max + min) / 2;
        const s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
        return { h, s: Math.round(s * 100), l: Math.round(l * 100) };
    });

    const hsv = $derived.by(() => {
        const r = rgb.r / 255, g = rgb.g / 255, b = rgb.b / 255;
        const max = Math.max(r, g, b), min = Math.min(r, g, b);
        const d = max - min;
        let h = 0;
        if (d !== 0) {
            if (max === r) h = ((g - b) / d) % 6;
            else if (max === g) h = (b - r) / d + 2;
            else h = (r - g) / d + 4;
            h = Math.round(h * 60);
            if (h < 0) h += 360;
        }
        const s = max === 0 ? 0 : d / max;
        return { h, s: Math.round(s * 100), v: Math.round(max * 100) };
    });

    const formats = $derived([
        { label: 'HEX', value: hex },
        { label: 'RGB', value: `rgb(${rgb.r}, ${rgb.g}, ${rgb.b})` },
        { label: 'HSL', value: `hsl(${hsl.h}, ${hsl.s}%, ${hsl.l}%)` },
        { label: 'HSV', value: `hsv(${hsv.h}, ${hsv.s}%, ${hsv.v}%)` },
    ]);

    // Readable text color over the current swatch (YIQ luminance).
    const swatchText = $derived(
        (rgb.r * 299 + rgb.g * 587 + rgb.b * 114) / 1000 >= 140 ? '#111111' : '#ffffff',
    );

    // ── Actions ───────────────────────────────────────────────────────────
    function setHex(value: string, remember = true) {
        const n = normalizeHex(value);
        if (!n) return;
        hex = n;
        if (remember) rememberColor(n);
    }

    function onHexInput(value: string) {
        const n = normalizeHex(value);
        if (n) hex = n; // live-update while typing; only remember on commit
    }

    async function pickFromScreen() {
        if (!EyeDropperImpl) return;
        try {
            const result = await new EyeDropperImpl().open();
            setHex(result.sRGBHex);
        } catch {
            // User pressed Esc / dismissed — not an error.
        }
    }

    async function copy(value: string) {
        try {
            await navigator.clipboard.writeText(value);
            copied = value;
            toast('Copied ' + value, 'success');
            setTimeout(() => {
                if (copied === value) copied = null;
            }, 1200);
        } catch {
            toast('Could not copy to clipboard', 'error');
        }
    }
</script>

<ToolPage
    icon={Pipette}
    iconTint="var(--color-accent)"
    title="Color Picker"
    description="Pick a color from anywhere on screen, or choose one manually, then copy it in any format."
>
    <div class="cp-grid">
        <!-- Swatch + source controls -->
        <div class="cp-swatch-col">
            <div class="cp-swatch" style="background: {hex}; color: {swatchText};">
                <span class="cp-swatch-hex">{hex}</span>
            </div>
            <div class="cp-controls">
                {#if hasEyedropper}
                    <Button variant="primary" icon={Pipette} onclick={pickFromScreen}>
                        Pick from screen
                    </Button>
                {/if}
                <label class="cp-native">
                    <span>Choose</span>
                    <input
                        type="color"
                        value={hex}
                        oninput={(e) => setHex((e.currentTarget as HTMLInputElement).value)}
                        aria-label="Choose a color"
                    />
                </label>
            </div>
            {#if !hasEyedropper}
                <p class="cp-hint">Screen eyedropper isn't available in this build — use the color box above.</p>
            {/if}
        </div>

        <!-- Formats -->
        <div class="cp-formats">
            <label class="cp-hex-field">
                <span class="cp-hex-label">HEX</span>
                <input
                    class="cp-hex-input"
                    type="text"
                    value={hex}
                    spellcheck="false"
                    oninput={(e) => onHexInput((e.currentTarget as HTMLInputElement).value)}
                    onchange={(e) => setHex((e.currentTarget as HTMLInputElement).value)}
                    aria-label="HEX value"
                />
            </label>
            {#each formats as fmt (fmt.label)}
                <button class="cp-fmt" type="button" onclick={() => copy(fmt.value)}>
                    <span class="cp-fmt-label">{fmt.label}</span>
                    <span class="cp-fmt-value">{fmt.value}</span>
                    {#if copied === fmt.value}
                        <Check class="cp-fmt-ico cp-fmt-ico-ok" />
                    {:else}
                        <Copy class="cp-fmt-ico" />
                    {/if}
                </button>
            {/each}
        </div>
    </div>

    {#if recent.length > 0}
        <div class="cp-recent">
            <span class="cp-recent-label">Recent</span>
            <div class="cp-recent-row">
                {#each recent as c (c)}
                    <button
                        class="cp-recent-swatch"
                        class:is-active={c.toUpperCase() === hex.toUpperCase()}
                        style="background: {c};"
                        title={c}
                        aria-label={'Use ' + c}
                        type="button"
                        onclick={() => setHex(c, false)}
                    ></button>
                {/each}
            </div>
        </div>
    {/if}
</ToolPage>

<style>
    .cp-grid {
        display: grid;
        grid-template-columns: minmax(220px, 280px) 1fr;
        gap: 1.25rem;
        align-items: start;
    }
    @media (max-width: 720px) {
        .cp-grid {
            grid-template-columns: 1fr;
        }
    }

    .cp-swatch-col {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
    }
    .cp-swatch {
        display: flex;
        align-items: flex-end;
        justify-content: flex-start;
        height: 180px;
        border-radius: 0.75rem;
        border: 1px solid var(--color-border);
        padding: 0.75rem;
        transition: background 0.12s ease;
    }
    .cp-swatch-hex {
        font-size: 1.1rem;
        font-weight: 600;
        font-variant-numeric: tabular-nums;
        letter-spacing: 0.02em;
    }
    .cp-controls {
        display: flex;
        align-items: center;
        gap: 0.6rem;
        flex-wrap: wrap;
    }
    .cp-native {
        display: inline-flex;
        align-items: center;
        gap: 0.4rem;
        font-size: 0.8rem;
        color: var(--color-muted);
    }
    .cp-native input[type='color'] {
        width: 2.2rem;
        height: 2.2rem;
        padding: 0;
        border: 1px solid var(--color-border);
        border-radius: 0.5rem;
        background: var(--color-panel-2);
        cursor: pointer;
    }
    .cp-hint {
        font-size: 0.78rem;
        color: var(--color-muted);
    }

    .cp-formats {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }
    .cp-hex-field {
        display: flex;
        align-items: center;
        gap: 0.6rem;
        margin-bottom: 0.25rem;
    }
    .cp-hex-label {
        width: 3rem;
        font-size: 0.72rem;
        font-weight: 600;
        color: var(--color-muted);
        text-transform: uppercase;
        letter-spacing: 0.04em;
    }
    .cp-hex-input {
        flex: 1;
        padding: 0.5rem 0.65rem;
        border-radius: 0.5rem;
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-family: var(--font-mono, monospace);
        font-size: 0.9rem;
    }
    .cp-hex-input:focus {
        outline: none;
        border-color: var(--color-accent);
    }

    .cp-fmt {
        display: flex;
        align-items: center;
        gap: 0.6rem;
        width: 100%;
        padding: 0.55rem 0.65rem;
        border-radius: 0.5rem;
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text);
        cursor: pointer;
        text-align: left;
        transition:
            border-color 0.12s ease,
            background 0.12s ease;
    }
    .cp-fmt:hover {
        border-color: var(--color-accent);
    }
    .cp-fmt-label {
        width: 2.4rem;
        font-size: 0.72rem;
        font-weight: 600;
        color: var(--color-muted);
        text-transform: uppercase;
        letter-spacing: 0.04em;
    }
    .cp-fmt-value {
        flex: 1;
        font-family: var(--font-mono, monospace);
        font-size: 0.9rem;
        font-variant-numeric: tabular-nums;
    }
    .cp-fmt :global(.cp-fmt-ico) {
        width: 1rem;
        height: 1rem;
        color: var(--color-muted);
        flex-shrink: 0;
    }
    .cp-fmt :global(.cp-fmt-ico-ok) {
        color: var(--color-accent);
    }

    .cp-recent {
        margin-top: 1.25rem;
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }
    .cp-recent-label {
        font-size: 0.72rem;
        font-weight: 600;
        color: var(--color-muted);
        text-transform: uppercase;
        letter-spacing: 0.04em;
    }
    .cp-recent-row {
        display: flex;
        flex-wrap: wrap;
        gap: 0.4rem;
    }
    .cp-recent-swatch {
        width: 1.9rem;
        height: 1.9rem;
        border-radius: 0.45rem;
        border: 1px solid var(--color-border);
        cursor: pointer;
        padding: 0;
        transition: transform 0.1s ease;
    }
    .cp-recent-swatch:hover {
        transform: translateY(-1px);
    }
    .cp-recent-swatch.is-active {
        box-shadow: 0 0 0 2px var(--color-accent);
    }
</style>
