<script lang="ts">
    /*
      Calculator — a Soulver-class calculating notepad. You type lines of math,
      units, percentages, and variables; a live result lands on every line, with
      a running total. All evaluation is local + offline (see stores/calcEngine).
      The standalone Unit Converter is folded in here (inline `X in Y`).
    */
    import { Calculator, Copy, Eraser, BookOpen } from '@lucide/svelte';
    import { ToolPage } from '$lib/ui';
    import { toast } from '$lib/stores/toasts';
    import { evaluateDocument } from '$lib/stores/calcEngine';
    import { utilCalculatorDoc } from '$lib/stores/utilCalculator';

    const PLACEHOLDER = `Type math, units and words — results appear on the right.

120 + 18%
20% of 80
10 km in miles
2 kg to lb
3 hours in minutes
today + 30 days

price = 1500
deposit = 20% of price
price - deposit
sum`;

    // The notepad persists across navigation via a module store (see
    // stores/utilCalculator). degrees/showHelp stay transient UI state.
    const doc = utilCalculatorDoc;
    let degrees = $state(true);
    let showHelp = $state(false);

    const results = $derived(evaluateDocument($doc, { degrees }));
    // Live total of the plain-number lines (unit/percent lines are excluded —
    // you can still total explicitly with the `sum` keyword).
    const total = $derived.by(() => {
        let acc = 0;
        let any = false;
        for (const r of results) {
            if (r.value && r.value.t === 'num') {
                acc += r.value.n;
                any = true;
            }
        }
        return any ? acc : null;
    });

    let editor: HTMLTextAreaElement | undefined;
    let gutter: HTMLDivElement | undefined;
    function syncScroll() {
        if (gutter && editor) gutter.scrollTop = editor.scrollTop;
    }

    function copyText(text: string) {
        if (!text) return;
        void navigator.clipboard.writeText(text.replace(/,/g, ''));
        toast('Copied', 'success');
    }
    function copyAll() {
        const lines = results.filter((r) => r.display).map((r) => r.display.replace(/,/g, ''));
        if (!lines.length) return;
        void navigator.clipboard.writeText(lines.join('\n'));
        toast(`Copied ${lines.length} result${lines.length === 1 ? '' : 's'}`, 'success');
    }
    function fmtTotal(n: number): string {
        return new Intl.NumberFormat('en-US', { maximumFractionDigits: 10 }).format(n);
    }

    const cheats: { e: string; r: string }[] = [
        { e: '120 + 18%', r: 'percentages' },
        { e: '20% of 80 · 15% off 200', r: 'of / off' },
        { e: '10 km in miles · 2 kg to lb', r: 'unit conversion' },
        { e: '100 c in f · 3 hours in minutes', r: 'temp / time' },
        { e: '5 ft + 3 in · 2 * 5 km', r: 'unit math' },
        { e: 'today + 30 days · 2026-03-01 - 2026-01-01', r: 'date math' },
        { e: 'x = 20  then  x * 3', r: 'variables' },
        { e: 'prev · sum · total', r: 'running totals' },
        { e: 'sqrt(9) · pow(2,8) · sin(30)', r: 'functions' },
    ];
</script>

<ToolPage
    icon={Calculator}
    iconTint="#f59e0b"
    title="Calculator"
    description="A calculating notepad — math, units, percentages and variables, all on-device."
    width="wide"
    fill={false}
>
    <!-- Toolbar -->
    <div class="flex flex-wrap items-center gap-2 rounded-2xl border border-border bg-panel p-3">
        <button
            onclick={() => (degrees = !degrees)}
            class="inline-flex h-9 items-center rounded-xl border border-border bg-panel-2 px-3 text-xs font-semibold hover:border-accent/70"
            title="Angle unit for trig functions"
        >
            {degrees ? 'DEG' : 'RAD'}
        </button>
        <div class="flex-1"></div>
        <button
            onclick={() => (showHelp = !showHelp)}
            class="inline-flex h-9 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70"
            aria-pressed={showHelp}
        >
            <BookOpen class="h-4 w-4" /> Examples
        </button>
        <button
            onclick={copyAll}
            class="inline-flex h-9 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70"
        >
            <Copy class="h-4 w-4" /> Copy all
        </button>
        <button
            onclick={() => doc.set('')}
            class="inline-flex h-9 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-error-strong"
        >
            <Eraser class="h-4 w-4" /> Clear
        </button>
    </div>

    {#if showHelp}
        <div class="rounded-2xl border border-border bg-panel p-4">
            <div class="mb-2 text-xs font-semibold uppercase tracking-wider text-muted">What you can type</div>
            <div class="grid gap-2 sm:grid-cols-2">
                {#each cheats as c}
                    <div class="flex items-baseline justify-between gap-3 rounded-lg bg-panel-2 px-3 py-2">
                        <code class="text-sm text-text">{c.e}</code>
                        <span class="shrink-0 text-[11px] text-muted">{c.r}</span>
                    </div>
                {/each}
            </div>
        </div>
    {/if}

    <!-- Paper: editable lines on the left, live results on the right -->
    <div class="calc-paper">
        <textarea
            bind:this={editor}
            bind:value={$doc}
            onscroll={syncScroll}
            wrap="off"
            spellcheck="false"
            placeholder={PLACEHOLDER}
            class="calc-input"
            aria-label="Calculator notepad"
        ></textarea>
        <div bind:this={gutter} class="calc-gutter" aria-hidden="true">
            {#each results as r}
                <div class="calc-row" class:is-err={r.kind === 'error'}>
                    {#if r.display}
                        <button class="calc-result" onclick={() => copyText(r.display)} title="Click to copy">{r.display}</button>
                    {:else if r.kind === 'error'}
                        <span class="calc-errdot" title={r.error}>!</span>
                    {:else}
                        <span class="calc-blank">&nbsp;</span>
                    {/if}
                </div>
            {/each}
        </div>
    </div>

    {#if total !== null}
        <div class="flex items-center justify-end gap-3 rounded-2xl border border-border bg-panel px-4 py-2.5 text-sm">
            <span class="text-muted">Σ Total of numbers</span>
            <button class="calc-result text-base font-semibold" onclick={() => copyText(fmtTotal(total))}>{fmtTotal(total)}</button>
        </div>
    {/if}
</ToolPage>

<style>
    .calc-paper {
        display: grid;
        grid-template-columns: minmax(0, 1fr) clamp(120px, 28%, 280px);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 16px);
        background: var(--color-panel);
        overflow: hidden;
    }
    /* The editor and the result gutter MUST share identical line metrics +
       top padding so row N lines up with result N. wrap=off keeps one visual
       line per logical line; syncScroll keeps them aligned while scrolling. */
    .calc-input,
    .calc-gutter {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 14px;
        line-height: 1.8;
        padding-top: 16px;
        padding-bottom: 64px;
    }
    .calc-input {
        min-height: 420px;
        max-height: 64vh;
        resize: none;
        white-space: pre;
        overflow: auto;
        padding-left: 18px;
        padding-right: 18px;
        background: transparent;
        color: var(--color-text);
        outline: none;
        border: none;
    }
    .calc-input::placeholder {
        color: var(--color-muted);
    }
    .calc-gutter {
        overflow: hidden;
        padding-left: 12px;
        padding-right: 14px;
        text-align: right;
        border-left: 1px solid var(--color-border);
        background: var(--color-panel-2);
    }
    .calc-row {
        height: 1.8em; /* one text line */
        overflow: hidden;
        white-space: nowrap;
    }
    .calc-result {
        max-width: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--color-accent);
        font-weight: 600;
        background: none;
        border: none;
        cursor: pointer;
    }
    .calc-result:hover {
        text-decoration: underline;
    }
    .calc-row.is-err .calc-errdot {
        display: inline-grid;
        place-items: center;
        width: 1.2em;
        height: 1.2em;
        border-radius: 4px;
        color: var(--color-error, #ef4444);
        font-weight: 700;
        cursor: help;
    }
    .calc-blank {
        opacity: 0;
    }
</style>
