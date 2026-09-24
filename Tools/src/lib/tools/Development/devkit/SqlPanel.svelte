<script lang="ts">
    import { AlertCircle, CheckCircle2, Copy, Lightbulb } from '@lucide/svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { get } from 'svelte/store';
    import { toast } from '$lib/stores/toasts';
    import { Button, ToolPanel } from '$lib/ui';
    // State lifted into a module store so a typed query + computed result
    // survive leaving and returning to Developer Tools (panels are remounted
    // fresh on navigation, wiping component-local $state).
    import {
        sqlInput,
        sqlOutput,
        sqlIndent,
        sqlUppercase,
        sqlLinesBetween,
        sqlPreset,
        sqlError,
        sqlLintError,
        sqlLintStats,
        sqlLintFindings,
        sqlLintCount,
        type SqlLintStat as LintStat,
        sqlProcessing,
        sqlLintRunning,
        type SqlLintFinding as LintFinding,
    } from '$lib/stores/devkitPanels';

    type Preset = {
        id: string;
        label: string;
        indent: number;
        uppercase: boolean;
        linesBetweenQueries: number;
        note: string;
    };

    // Ephemeral UI flags stay component-local — cheap to recompute, no need to
    // survive navigation.
    let processing = $derived($sqlProcessing);
    let lintRunning = $derived($sqlLintRunning);

    const presets: Preset[] = [
        { id: 'readable', label: 'Readable', indent: 2, uppercase: true, linesBetweenQueries: 2, note: 'Balanced readability for code review.' },
        { id: 'compact', label: 'Compact', indent: 2, uppercase: false, linesBetweenQueries: 1, note: 'Short lines and smaller gaps.' },
        { id: 'audit', label: 'Audit', indent: 4, uppercase: true, linesBetweenQueries: 1, note: 'Clear statement boundaries for inspection.' },
        { id: 'minified', label: 'Diff-friendly', indent: 2, uppercase: false, linesBetweenQueries: 0, note: 'Smaller diff noise when comparing versions.' },
    ];

    const currentPreset = $derived.by(() => presets.find((item) => item.id === $sqlPreset) ?? presets[0]);

    let timeout: ReturnType<typeof setTimeout>;

    function applyPreset(id: string) {
        $sqlPreset = id;
        const selected = presets.find((item) => item.id === id);
        if (!selected) return;
        $sqlIndent = selected.indent;
        $sqlUppercase = selected.uppercase;
        $sqlLinesBetween = selected.linesBetweenQueries;
    }

    // SB-4 (2026-05-29): the old regex helpers (cleanSql / extractTables /
    // extractWhereColumns) that powered the fake "Explain" were removed —
    // analysis now goes through the sqlparser-backed `analyze_sql` command.

    // SB-4: real SQL lint via the sqlparser-backed Rust command. Honest
    // static analysis (syntax validation, statement classification, and
    // the UPDATE/DELETE-without-WHERE safety check) — NOT a fake "query
    // plan". The old regex heuristics are gone.
    async function analyzeSql(sql: string): Promise<void> {
        if (lintRunning) return;
        if (!sql.trim()) {
            $sqlLintStats = [];
            $sqlLintFindings = [];
            $sqlLintCount = 0;
            $sqlLintError = null;
            return;
        }
        $sqlLintRunning = true;
        $sqlLintError = null;
        try {
            const res = await invoke<{
                statementCount: number;
                valid: boolean;
                stats: LintStat[];
                findings: LintFinding[];
                parseError: string | null;
            }>('analyze_sql', { options: { input: sql } });
            $sqlLintCount = res.statementCount;
            $sqlLintStats = res.stats;
            $sqlLintFindings = res.findings;
            $sqlLintError = null;
        } catch (e) {
            $sqlLintError = String(e);
            $sqlLintStats = [];
            $sqlLintFindings = [];
            $sqlLintCount = 0;
        } finally {
            $sqlLintRunning = false;
        }
    }

    async function run() {
        if (processing) return;
        if (!get(sqlInput).trim()) {
            $sqlOutput = '';
            $sqlError = null;
            return;
        }
        $sqlProcessing = true;
        $sqlError = null;
        try {
            const result = await invoke<{ formatted: string; error: string | null }>('format_sql', {
                options: {
                    input: get(sqlInput),
                    indent: get(sqlIndent),
                    uppercase: get(sqlUppercase),
                    lines_between_queries: get(sqlLinesBetween),
                },
            });
            $sqlOutput = result.formatted;
            $sqlError = result.error;
        } catch (e) {
            $sqlError = String(e);
        } finally {
            $sqlProcessing = false;
        }
    }

    $effect(() => {
        $sqlInput;
        $sqlIndent;
        $sqlUppercase;
        $sqlLinesBetween;
        clearTimeout(timeout);
        timeout = setTimeout(() => {
            run();
        }, 200);
    });

    function minify() {
        if (!$sqlOutput) return;
        $sqlOutput = $sqlOutput.replace(/\s+/g, ' ').trim();
        toast('Minified result', 'success');
    }

    async function copyOutput() {
        if ($sqlOutput) {
            await navigator.clipboard.writeText($sqlOutput);
            toast('Copied formatted SQL', 'success');
        }
    }

    async function copyLint() {
        const text = [
            `SQL lint — ${$sqlLintCount} statement(s)`,
            ...$sqlLintStats.map((row) => `${row.label}: ${row.value}`),
            'Findings:',
            ...$sqlLintFindings.map((f) => `- [${f.severity}] ${f.message}`),
        ].join('\n');
        await navigator.clipboard.writeText(text);
        toast('Copied lint report', 'success');
    }

    function clear() {
        $sqlInput = '';
        $sqlOutput = '';
        $sqlError = null;
        $sqlLintStats = [];
        $sqlLintFindings = [];
        $sqlLintCount = 0;
        $sqlLintError = null;
    }
</script>

<div class="dt-panel">
    <div class="dt-head">
        <div class="dt-head-text">
            <h2 class="dt-title">SQL Formatter — format, then lint for real</h2>
            <p class="dt-desc">Pretty-print queries with preset packs, then run a real parser-backed lint that validates syntax, classifies statements, and flags dangerous writes like UPDATE/DELETE with no WHERE. Static analysis, not a query plan.</p>
        </div>
    </div>

    <!-- Toolbar -->
    <ToolPanel padding="md">
        <div class="sql-toolbar">
            <div class="sql-toolbar-row">
                <div class="dt-seg" role="group" aria-label="Preset">
                    {#each presets as item}
                        <button class="dt-seg-btn" class:is-active={$sqlPreset === item.id} onclick={() => applyPreset(item.id)}>
                            {item.label}
                        </button>
                    {/each}
                </div>
                <span class="dt-hint sql-toolbar-end">Preset: {currentPreset.note}</span>
            </div>

            <div class="sql-controls">
                <div class="sql-control">
                    <span class="dt-label">Indent</span>
                    <div class="dt-seg" role="group" aria-label="Indent">
                        {#each [2, 4] as n}
                            <button class="dt-seg-btn" class:is-active={$sqlIndent === n} onclick={() => ($sqlIndent = n)}>
                                {n} spaces
                            </button>
                        {/each}
                    </div>
                </div>

                <div class="sql-control">
                    <span class="dt-label">Keywords</span>
                    <div class="dt-seg" role="group" aria-label="Keywords">
                        <button class="dt-seg-btn" class:is-active={$sqlUppercase} onclick={() => ($sqlUppercase = true)}>
                            UPPERCASE
                        </button>
                        <button class="dt-seg-btn" class:is-active={!$sqlUppercase} onclick={() => ($sqlUppercase = false)}>
                            lowercase
                        </button>
                    </div>
                </div>

                <div class="sql-control">
                    <label for="sql-blank-lines" class="dt-label">Blank lines: {$sqlLinesBetween}</label>
                    <input
                        id="sql-blank-lines"
                        type="range"
                        min="0"
                        max="3"
                        bind:value={$sqlLinesBetween}
                        class="sql-range"
                    />
                </div>

                <div class="sql-control-spacer"></div>

                <div class="sql-actions">
                    <Button
                        variant="secondary"
                        icon={Lightbulb}
                        loading={lintRunning}
                        disabled={!$sqlInput.trim() || lintRunning}
                        onclick={() => analyzeSql($sqlInput)}
                    >
                        {lintRunning ? 'Linting…' : 'Lint'}
                    </Button>
                    <Button variant="secondary" disabled={!$sqlOutput} onclick={minify}>
                        Minify result
                    </Button>
                    <Button variant="ghost" onclick={clear}>
                        Clear
                    </Button>
                    <Button variant="primary" icon={Copy} disabled={!$sqlOutput} onclick={copyOutput}>
                        Copy output
                    </Button>
                </div>
            </div>
        </div>
    </ToolPanel>

    <!-- Editor + output -->
    <div class="dt-two">
        <div class="dt-io">
            <div class="dt-io-head">
                <span class="dt-section-label">Input</span>
                <span class="dt-hint">{$sqlInput.length} chars</span>
            </div>
            <textarea
                id="sql-in"
                class="dt-ta is-tall"
                bind:value={$sqlInput}
                spellcheck="false"
                placeholder="Paste SQL here..."
            ></textarea>
        </div>

        <div class="dt-io">
            <div class="dt-io-head">
                <span class="dt-section-label">Formatted</span>
                <span class="dt-hint">{$sqlOutput.length} chars</span>
            </div>
            <textarea
                class="dt-ta is-tall"
                class:is-error={$sqlError}
                value={$sqlOutput}
                readonly
                spellcheck="false"
            ></textarea>
        </div>
    </div>

    <div class="dt-two">
        <ToolPanel padding="md">
            <div class="dt-io-head">
                <span class="dt-section-label">SQL Lint{$sqlLintCount > 0 ? ` — ${$sqlLintCount} statement(s)` : ''}</span>
            </div>
            <div class="sql-card-body">
                <div class="sql-lint-note">
                    <Lightbulb class="sql-note-ico" style="color:var(--color-accent)" />
                    <span class="dt-hint">Parser-backed static analysis · not a database query plan</span>
                </div>
                {#if $sqlLintError}
                    <div class="dt-error">{$sqlLintError}</div>
                {:else if $sqlLintStats.length === 0}
                    <div class="dt-hint">Click <span class="sql-emph">Lint</span> to parse and check this SQL.</div>
                {:else}
                    <div class="sql-stats">
                        {#each $sqlLintStats as stat}
                            <div class="dt-kv">
                                <span class="dt-kv-key">{stat.label}</span>
                                <span class="dt-kv-val">{stat.value}</span>
                            </div>
                        {/each}
                    </div>
                {/if}
            </div>
        </ToolPanel>

        <ToolPanel padding="md">
            <div class="dt-io-head">
                <span class="dt-section-label">Findings</span>
                {#if $sqlLintFindings.length > 0}
                    <Button variant="ghost" size="sm" icon={Copy} onclick={copyLint}>Copy report</Button>
                {/if}
            </div>
            <div class="sql-findings">
                {#if $sqlLintFindings.length === 0}
                    <div class="dt-hint">No findings yet — run Lint.</div>
                {:else}
                    {#each $sqlLintFindings as f}
                        <div
                            class="sql-finding {f.severity === 'high' ? 'dt-text-error' : f.severity === 'info' ? 'dt-text-success' : f.severity === 'medium' ? '' : 'dt-text-muted'}"
                            style={f.severity === 'medium' ? 'color:var(--color-accent)' : undefined}
                        >
                            {#if f.severity === 'high'}
                                <AlertCircle class="sql-finding-ico" />
                            {:else if f.severity === 'info'}
                                <CheckCircle2 class="sql-finding-ico" />
                            {:else}
                                <Lightbulb class="sql-finding-ico" />
                            {/if}
                            <span>{f.message}</span>
                        </div>
                    {/each}
                {/if}
            </div>
        </ToolPanel>
    </div>

    {#if $sqlError}
        <div class="dt-error">{$sqlError}</div>
    {/if}
</div>

<style>
    /* Range accent: the one scoped style allowed for this panel. */
    .sql-range {
        width: 8rem;
        accent-color: var(--color-accent);
    }

    /* Layout-only helpers (no color tokens) for the recomposed chrome. */
    .sql-toolbar {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .sql-toolbar-row {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
    }
    .sql-toolbar-end {
        margin-left: auto;
    }
    .sql-controls {
        display: flex;
        flex-wrap: wrap;
        align-items: flex-end;
        gap: 16px;
    }
    .sql-control {
        display: flex;
        flex-direction: column;
        gap: 5px;
    }
    .sql-control-spacer {
        flex: 1;
    }
    .sql-actions {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
    }
    .sql-card-body {
        margin-top: 10px;
    }
    .sql-lint-note {
        display: flex;
        align-items: center;
        gap: 6px;
        margin-bottom: 8px;
    }
    .sql-lint-note :global(.sql-note-ico) {
        width: 14px;
        height: 14px;
    }
    .sql-emph {
        color: var(--color-text);
    }
    .sql-stats {
        display: grid;
        gap: 4px;
    }
    .sql-findings {
        display: flex;
        flex-direction: column;
        gap: 6px;
        margin-top: 10px;
        font-size: 12px;
    }
    .sql-finding {
        display: flex;
        align-items: flex-start;
        gap: 6px;
    }
    .sql-finding :global(.sql-finding-ico) {
        width: 14px;
        height: 14px;
        flex-shrink: 0;
        margin-top: 2px;
    }
</style>
