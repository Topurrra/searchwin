<script lang="ts">
    import { AlertTriangle, ArrowLeftRight, GitCompare, Copy } from '@lucide/svelte';
    import { get } from 'svelte/store';
    import { toast } from '$lib/stores/toasts';
    import { escToClear } from '$lib/actions/escToClear';
    import { Button, Checkbox, ToolPanel, EmptyState } from '$lib/ui';
    // State lifted into a module store so typed inputs + the computed diff
    // survive leaving and returning to Developer Tools (panels remount fresh on
    // navigation).
    import {
        diffLeftLabel,
        diffRightLabel,
        diffLeftText,
        diffRightText,
        diffMode,
        diffIgnoreWhitespace,
        diffIgnoreCase,
        diffPiiAware,
        diffRevealPii,
        diffComputed,
        type DiffRow,
    } from '$lib/stores/devkitPanels';

    // FREEZE GUARD (Concern B): diffLines is an O(n·m) LCS that allocates a
    // full (n+1)×(m+1) matrix. On a large paste this hangs the whole UI. Two
    // mitigations: (1) the diff runs in a DEBOUNCED effect ~200ms after typing
    // stops, not on every keystroke; (2) when either side exceeds the cap we
    // skip the auto-diff and require an explicit "Compare" press.
    const DIFF_DEBOUNCE_MS = 200;
    const MAX_DIFF_LINES = 5_000;
    const MAX_DIFF_CHARS = 500_000;

    type PiiRule = {
        id: string;
        regex: RegExp;
        label: string;
    };

    const piiRules: PiiRule[] = [
        {
            id: 'email',
            label: 'Email',
            regex: /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g,
        },
        {
            id: 'phone',
            label: 'Phone',
            regex: /\b(?:\+?\d{1,3}[-. ]?)?(?:\(\d{1,4}\)|\d{1,4})[-. ]?\d{3,4}[-. ]?\d{4}\b/g,
        },
        {
            id: 'ssn',
            label: 'SSN',
            regex: /\b\d{3}-\d{2}-\d{4}\b/g,
        },
        {
            id: 'jwt',
            label: 'JWT',
            regex: /\b[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b/g,
        },
        {
            id: 'card',
            label: 'Card',
            regex: /\b(?:\d[ -]*?){13,19}\b/g,
        },
        {
            id: 'api',
            label: 'API/secret',
            regex: /\b(api[_-]?key|secret|token|password)\s*[:=]\s*["']?[A-Za-z0-9+/_=]{16,}["']?/gi,
        },
    ];

    function normalize(s: string): string {
        if ($diffIgnoreCase) s = s.toLowerCase();
        if ($diffIgnoreWhitespace) s = s.replace(/\s+/g, ' ').trim();
        return s;
    }

    const piiReplace = '<PII_REDACTED>';

    function applyPiiMask(s: string): string {
        if (!$diffPiiAware) return s;
        if ($diffRevealPii) return s;
        let out = s;
        for (const rule of piiRules) {
            out = out.replace(rule.regex, piiReplace);
        }
        return out;
    }

    let piiCounts = $derived.by(() => {
        const all = `${$diffLeftText}\n${$diffRightText}`;
        const out: Record<string, number> = {};
        for (const rule of piiRules) {
            const count = (all.match(rule.regex) ?? []).length;
            if (count > 0) {
                out[rule.id] = count;
            }
        }
        return out;
    });

    // Patience-like LCS for line diff. Good enough for typical text sizes.
    function diffLines(a: string[], b: string[]): DiffRow[] {
        const n = a.length;
        const m = b.length;
        const dp: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));

        for (let i = 1; i <= n; i++) {
            for (let j = 1; j <= m; j++) {
                if (normalize(a[i - 1]) === normalize(b[j - 1])) {
                    dp[i][j] = dp[i - 1][j - 1] + 1;
                } else {
                    dp[i][j] = Math.max(dp[i - 1][j], dp[i][j - 1]);
                }
            }
        }

        // Backtrack
        const rows: DiffRow[] = [];
        let i = n, j = m;
        while (i > 0 && j > 0) {
            if (normalize(a[i - 1]) === normalize(b[j - 1])) {
                rows.unshift({ op: 'eq', leftLine: i, leftText: a[i - 1], rightLine: j, rightText: b[j - 1] });
                i--;
                j--;
            } else if (dp[i - 1][j] >= dp[i][j - 1]) {
                rows.unshift({ op: 'del', leftLine: i, leftText: a[i - 1] });
                i--;
            } else {
                rows.unshift({ op: 'ins', rightLine: j, rightText: b[j - 1] });
                j--;
            }
        }
        while (i > 0) {
            rows.unshift({ op: 'del', leftLine: i, leftText: a[i - 1] });
            i--;
        }
        while (j > 0) {
            rows.unshift({ op: 'ins', rightLine: j, rightText: b[j - 1] });
            j--;
        }

        // Pair adjacent del+ins as 'mod' for clearer side-by-side
        const merged: DiffRow[] = [];
        let k = 0;
        while (k < rows.length) {
            if (rows[k].op === 'del' && k + 1 < rows.length && rows[k + 1].op === 'ins') {
                merged.push({
                    op: 'mod',
                    leftLine: rows[k].leftLine,
                    leftText: rows[k].leftText,
                    rightLine: rows[k + 1].rightLine,
                    rightText: rows[k + 1].rightText,
                });
                k += 2;
            } else {
                merged.push(rows[k]);
                k++;
            }
        }
        return merged;
    }

    // Rows are read from the store, written by the debounced effect / Compare
    // button below. Reading `$diffComputed` keeps the template reactive.
    let rows = $derived($diffComputed.rows);

    /** True when either side is too big to auto-diff (see the cap constants). */
    function exceedsCap(left: string, right: string): boolean {
        if (left.length > MAX_DIFF_CHARS || right.length > MAX_DIFF_CHARS) return true;
        // Counting newlines is cheaper than split() for the cap check.
        let leftLines = 1, rightLines = 1;
        for (let i = 0; i < left.length; i++) if (left.charCodeAt(i) === 10) leftLines++;
        for (let i = 0; i < right.length; i++) if (right.charCodeAt(i) === 10) rightLines++;
        return leftLines > MAX_DIFF_LINES || rightLines > MAX_DIFF_LINES;
    }

    /** Run the actual LCS diff and write the result into the store. Reads the
     *  current store values at call time so it works from both the debounced
     *  effect and the explicit Compare button. */
    function computeDiff(): void {
        const left = get(diffLeftText);
        const right = get(diffRightText);
        const leftLines = left.split('\n').map((line) => applyPiiMask(line));
        const rightLines = right.split('\n').map((line) => applyPiiMask(line));
        diffComputed.set({ rows: diffLines(leftLines, rightLines), tooLarge: false });
    }

    // Debounced auto-diff: re-runs ~200ms after the inputs / options settle.
    // When the inputs are over the cap we DON'T diff automatically — we flag
    // `tooLarge` so the template shows a notice + Compare button instead of
    // hanging the UI on a quadratic matrix.
    let debounceTimer: ReturnType<typeof setTimeout>;
    $effect(() => {
        // Track every input that affects the diff so the effect re-runs.
        const left = $diffLeftText;
        const right = $diffRightText;
        $diffIgnoreWhitespace;
        $diffIgnoreCase;
        $diffPiiAware;
        $diffRevealPii;

        clearTimeout(debounceTimer);
        if (exceedsCap(left, right)) {
            // Park: clear rows, mark too-large, wait for an explicit Compare.
            diffComputed.set({ rows: [], tooLarge: true });
            return;
        }
        debounceTimer = setTimeout(computeDiff, DIFF_DEBOUNCE_MS);
        return () => clearTimeout(debounceTimer);
    });

    /** Explicit "Compare" for over-cap inputs — the user opted in, so we run
     *  the diff once even though it may be heavy. */
    function compareNow(): void {
        clearTimeout(debounceTimer);
        computeDiff();
    }

    let stats = $derived.by(() => {
        let added = 0, removed = 0, modified = 0, unchanged = 0;
        for (const r of rows) {
            if (r.op === 'ins') added++;
            else if (r.op === 'del') removed++;
            else if (r.op === 'mod') modified++;
            else unchanged++;
        }
        return { added, removed, modified, unchanged };
    });

    let piiSummary = $derived.by(() =>
        Object.entries(piiCounts).map(([id, count]) => `${id}: ${count}`).join(' · '),
    );

    function swap() {
        const l = get(diffLeftText);
        const r = get(diffRightText);
        diffLeftText.set(r);
        diffRightText.set(l);
        const ll = get(diffLeftLabel);
        const rl = get(diffRightLabel);
        diffLeftLabel.set(rl);
        diffRightLabel.set(ll);
    }

    function outputLineForRow(line: string | undefined, fallbackLines: string[], index: number | undefined): string {
        if (line !== undefined) return line;
        if (index === undefined) return '';
        const raw = fallbackLines[index - 1];
        return raw !== undefined ? ($diffPiiAware ? applyPiiMask(raw) : raw) : '';
    }

    // Unified text export
    function unifiedDiff(): string {
        let out = `--- ${$diffLeftLabel}\n+++ ${$diffRightLabel}\n`;
        const leftLines = $diffLeftText.split('\n');
        const rightLines = $diffRightText.split('\n');
        for (const r of rows) {
            if (r.op === 'eq') out += `  ${outputLineForRow(r.leftText, leftLines, r.leftLine)}\n`;
            else if (r.op === 'del') out += `- ${outputLineForRow(r.leftText, leftLines, r.leftLine)}\n`;
            else if (r.op === 'ins') out += `+ ${outputLineForRow(r.rightText, rightLines, r.rightLine)}\n`;
            else if (r.op === 'mod') {
                out += `- ${outputLineForRow(r.leftText, leftLines, r.leftLine)}\n`;
                out += `+ ${outputLineForRow(r.rightText, rightLines, r.rightLine)}\n`;
            }
        }
        return out;
    }

    async function copyUnified() {
        await navigator.clipboard.writeText(unifiedDiff());
        toast('Copied unified diff', 'success');
    }
</script>

<div class="dt-panel">
    <div class="dt-head">
        <div class="dt-head-text">
            <h2 class="dt-title">Diff Viewer — compare text side-by-side</h2>
            <p class="dt-desc">Paste two text blocks and see the differences, unified or split. Useful before sending patches or comparing configs. Nothing is uploaded.</p>
        </div>
        <div class="dt-seg" role="group" aria-label="Diff layout">
            <button
                type="button"
                class="dt-seg-btn"
                class:is-active={$diffMode === 'side'}
                onclick={() => ($diffMode = 'side')}
            >
                Side-by-side
            </button>
            <button
                type="button"
                class="dt-seg-btn"
                class:is-active={$diffMode === 'unified'}
                onclick={() => ($diffMode = 'unified')}
            >
                Unified
            </button>
        </div>
    </div>

    <!-- Toolbar -->
    <div class="dt-toolbar">
        <Checkbox bind:checked={$diffIgnoreWhitespace} label="Ignore whitespace" />
        <Checkbox bind:checked={$diffIgnoreCase} label="Ignore case" />
        <Checkbox bind:checked={$diffPiiAware} label="PII-aware mode" />
        <Checkbox bind:checked={$diffRevealPii} label="Reveal PII" />

        <Button variant="secondary" size="sm" icon={ArrowLeftRight} onclick={swap}>Swap</Button>

        <div class="diff-stats dt-mono">
            <span class="diff-add-text">+{stats.added}</span>
            <span class="diff-sep">·</span>
            <span class="diff-del-text">- {stats.removed}</span>
            <span class="diff-sep">·</span>
            <span class="diff-mod-text">~ {stats.modified}</span>
            <span class="diff-sep">·</span>
            <span>={stats.unchanged}</span>
        </div>

        <div class="dt-toolbar-end">
            <Button variant="primary" size="sm" icon={Copy} onclick={copyUnified}>Copy unified</Button>
        </div>
    </div>

    {#if $diffPiiAware}
        <div class="dt-note">
            <span class="diff-note-key">PII indicators:</span>
            {#if piiSummary}
                <span class="diff-note-val">{piiSummary}</span>
            {:else}
                <span class="diff-note-val">No obvious direct matches in current inputs.</span>
            {/if}
        </div>
    {/if}

    {#if $diffPiiAware && Object.keys(piiCounts).length > 0 && !$diffRevealPii}
        <div class="dt-error diff-mask-banner">
            <AlertTriangle class="diff-banner-ico" />
            PII masking is active. Enable reveal to inspect full text.
        </div>
    {/if}

    <!-- Inputs -->
    <div class="dt-two">
        <div class="dt-io">
            <div class="dt-io-head">
                <span class="dt-section-label">Original</span>
            </div>
            <input
                type="text"
                bind:value={$diffLeftLabel}
                use:escToClear={() => ($diffLeftLabel = '')}
                class="dt-input diff-label-input"
                aria-label="Original label"
            />
            <textarea
                class="dt-ta is-tall"
                bind:value={$diffLeftText}
                spellcheck="false"
            ></textarea>
        </div>
        <div class="dt-io">
            <div class="dt-io-head">
                <span class="dt-section-label">Modified</span>
            </div>
            <input
                type="text"
                bind:value={$diffRightLabel}
                use:escToClear={() => ($diffRightLabel = '')}
                class="dt-input diff-label-input"
                aria-label="Modified label"
            />
            <textarea
                class="dt-ta is-tall"
                bind:value={$diffRightText}
                spellcheck="false"
            ></textarea>
        </div>
    </div>

    <!-- Diff render -->
    <ToolPanel padding="md">
        {#if !$diffLeftText.trim() && !$diffRightText.trim()}
            <EmptyState
                icon={GitCompare}
                title="Nothing to compare yet"
                description="Paste text into both editors above and the differences appear here — side-by-side or unified."
                variant="compact"
            />
        {:else if $diffComputed.tooLarge}
            <!-- FREEZE GUARD: inputs over the cap. Auto-diff is skipped so the
                 UI never hangs on a quadratic matrix — the user opts in. -->
            <div class="diff-out diff-toolarge">
                <AlertTriangle class="diff-banner-ico" />
                <div class="diff-toolarge-text">
                    <strong>Input is large.</strong>
                    Auto-comparison is paused above {MAX_DIFF_LINES.toLocaleString()} lines
                    or {Math.round(MAX_DIFF_CHARS / 1000)}k characters to keep the app
                    responsive. Press Compare to run it once.
                </div>
                <Button variant="primary" size="sm" icon={GitCompare} onclick={compareNow}>Compare</Button>
            </div>
        {:else}
            <div class="diff-out">
                <div class="diff-out-head dt-section-label">
                    <span>{$diffLeftLabel}</span>
                    {#if $diffMode === 'side'}<span>{$diffRightLabel}</span>{/if}
                </div>

                <div class="diff-scroll">
                    {#if $diffMode === 'side'}
                        <table class="diff-table dt-mono">
                            <tbody>
                            {#each rows as r}
                                <tr class="diff-tr">
                                    <!-- LEFT SIDE -->
                                    <td class="diff-gutter diff-gutter-l">
                                        {r.leftLine ?? ''}
                                    </td>
                                    <td class="diff-cell" class:diff-del={r.op === 'del' || r.op === 'mod'}>
                                        {r.leftLine ? outputLineForRow(r.leftText, $diffLeftText.split('\n'), r.leftLine) : ''}
                                    </td>
                                    <!-- RIGHT SIDE -->
                                    <td class="diff-gutter diff-gutter-r">
                                        {r.rightLine ?? ''}
                                    </td>
                                    <td class="diff-cell" class:diff-add={r.op === 'ins' || r.op === 'mod'}>
                                        {r.rightLine ? outputLineForRow(r.rightText, $diffRightText.split('\n'), r.rightLine) : ''}
                                    </td>
                                </tr>
                            {/each}
                            </tbody>
                        </table>
                    {:else}
                        <div class="diff-unified dt-mono">
                            {#each rows as r}
                                {#if r.op === 'eq'}
                                    <div class="diff-line diff-ctx">  {outputLineForRow(r.leftText, $diffLeftText.split('\n'), r.leftLine)}</div>
                                {:else if r.op === 'del'}
                                    <div class="diff-line diff-del">- {outputLineForRow(r.leftText, $diffLeftText.split('\n'), r.leftLine)}</div>
                                {:else if r.op === 'ins'}
                                    <div class="diff-line diff-add">+ {outputLineForRow(r.rightText, $diffRightText.split('\n'), r.rightLine)}</div>
                                {:else}
                                    <div class="diff-line diff-del">- {outputLineForRow(r.leftText, $diffLeftText.split('\n'), r.leftLine)}</div>
                                    <div class="diff-line diff-add">+ {outputLineForRow(r.rightText, $diffRightText.split('\n'), r.rightLine)}</div>
                                {/if}
                            {/each}
                        </div>
                    {/if}
                </div>
            </div>
        {/if}
    </ToolPanel>
</div>

<style>
    /* Diff coloring: semantic tokens only, no hardcoded hex.
       added -> success, removed -> error, modified pairs reuse both,
       context -> muted. The colored line classes get a faint token-mix
       background so the eye lands on the change, not just the glyph. */

    .diff-stats {
        font-size: 11px;
        color: var(--color-muted);
    }
    .diff-sep {
        margin: 0 4px;
    }
    .diff-add-text {
        color: var(--color-success);
    }
    .diff-del-text {
        color: var(--color-error);
    }
    .diff-mod-text {
        color: var(--color-warning);
    }

    .diff-note-key {
        color: var(--color-text);
    }
    .diff-note-val {
        margin-left: 6px;
    }

    .diff-mask-banner {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: 12px;
    }
    .diff-mask-banner :global(.diff-banner-ico) {
        width: 14px;
        height: 14px;
        flex: none;
    }

    /* Over-cap notice: input too large to auto-diff (freeze guard). */
    .diff-toolarge {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 16px;
        font-size: 12.5px;
        color: var(--color-text-secondary);
    }
    .diff-toolarge :global(.diff-banner-ico) {
        width: 20px;
        height: 20px;
        flex: none;
        color: var(--color-warning);
    }
    .diff-toolarge-text {
        flex: 1;
        min-width: 0;
        line-height: 1.5;
    }
    .diff-toolarge-text strong {
        color: var(--color-text);
    }

    .diff-label-input {
        height: 28px;
        font-size: 11.5px;
        font-weight: 500;
    }

    /* Output frame */
    .diff-out {
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
        overflow: hidden;
    }
    .diff-out-head {
        display: flex;
        justify-content: space-between;
        gap: 8px;
        padding: 8px 12px;
        border-bottom: 1px solid var(--color-border);
    }
    .diff-scroll {
        max-height: 60vh;
        overflow: auto;
    }

    /* Side-by-side table */
    .diff-table {
        width: 100%;
        font-size: 12px;
        border-collapse: collapse;
    }
    .diff-tr:hover {
        background: color-mix(in srgb, var(--color-text) 4%, transparent);
    }
    .diff-gutter {
        width: 2.5rem;
        padding: 1px 8px;
        text-align: right;
        color: var(--color-muted);
        user-select: none;
    }
    .diff-gutter-l {
        border-right: 1px solid var(--color-border);
    }
    .diff-gutter-r {
        border-left: 1px solid var(--color-border);
    }
    .diff-cell {
        width: 50%;
        padding: 1px 8px;
        white-space: pre-wrap;
        word-break: break-all;
    }

    /* Unified column */
    .diff-unified {
        font-size: 12px;
    }
    .diff-line {
        padding: 1px 12px;
        white-space: pre-wrap;
        word-break: break-all;
    }
    .diff-ctx {
        color: var(--color-muted);
    }

    /* Semantic change colors (shared by table cells + unified lines) */
    .diff-add {
        color: var(--color-success);
        background: color-mix(in srgb, var(--color-success) 8%, transparent);
    }
    .diff-del {
        color: var(--color-error);
        background: color-mix(in srgb, var(--color-error) 8%, transparent);
    }
</style>
