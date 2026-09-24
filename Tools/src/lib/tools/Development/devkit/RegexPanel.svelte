<script lang="ts">
    /*
      Regex Tool — an offline regex101: build a pattern from clickable blocks
      or example strings (grex), test it live with highlighted matches + capture
      groups, see a plain-English breakdown of every token, and preview a
      find/replace. ECMAScript flavor (the engine your JS actually runs).
    */
    import { Copy, Regex as RegexIcon, Sparkles } from '@lucide/svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { get } from 'svelte/store';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import { Button, ToolPanel, Checkbox, EmptyState } from '$lib/ui';
    import { explainPattern, type RegexTokenType } from '$lib/stores/regexExplain';
    // State lifted into a module store so a typed pattern + test string +
    // computed matches survive leaving and returning to Developer Tools (panels
    // remount fresh on navigation).
    import {
        regexTab,
        regexPattern,
        regexTestString,
        regexReplacement,
        regexFlags,
        regexExampleInput,
        regexComputed,
        type RegexMatch as Match,
    } from '$lib/stores/devkitPanels';

    // FREEZE GUARD (Concern B): the pattern is compiled to RegExp and run in a
    // synchronous `while(re.exec(...))` loop. A catastrophic-backtracking
    // pattern (e.g. `(a+)+$`) against a long string would hang the whole app.
    // JS regex can't be interrupted mid-exec, so the practical mitigations are
    // (1) DEBOUNCE so we don't recompute on every keystroke, and (2) CAP the
    // live-matched test string length — over the cap we match only the first
    // N chars and surface a small inline "truncated" notice.
    const REGEX_DEBOUNCE_MS = 200;
    const MAX_LIVE_MATCH_CHARS = 100_000;

    const SUPPORTED_FLAGS = new Set(['g', 'i', 'm', 's', 'u', 'y', 'd']);

    /** Plain-English error, and a special case for inline flag/group syntax
     *  that JavaScript's RegExp can't parse (the #1 cross-language gotcha). */
    function friendlyError(e: unknown, pat: string): string {
        const msg = e instanceof Error ? e.message : String(e);
        if (/invalid group/i.test(msg) && /\(\?/.test(pat)) {
            return 'JavaScript regex does not support this inline group/flag syntax (e.g. (?i) or Python-style (?P<name>)). Use the i / m / s checkboxes above, and (?<name>...) for named groups.';
        }
        return msg;
    }

    /** Compile the current pattern + flags into a RegExp (or an error). Pure —
     *  called from the debounced compute below. */
    function compileRegex(pattern: string, flags: { g: boolean; i: boolean; m: boolean; s: boolean; u: boolean }) {
        if (!pattern) return { re: null as RegExp | null, error: null as string | null };
        let pat = pattern;
        let lifted = '';
        // Lift a leading inline-flag group like (?i) / (?ims) into RegExp flags.
        // ECMAScript has no inline flag groups, but many patterns are pasted from
        // other tools/languages — quietly honoring a leading one keeps them working.
        const m = /^\(\?([a-z]+)\)/.exec(pat);
        if (m && [...m[1]].every((c) => SUPPORTED_FLAGS.has(c))) {
            lifted = m[1];
            pat = pat.slice(m[0].length);
        }
        try {
            const active = Object.entries(flags).filter(([, v]) => v).map(([k]) => k);
            const f = [...new Set([...active, ...lifted])].join('');
            return { re: new RegExp(pat, f), error: null };
        } catch (e) {
            return { re: null, error: friendlyError(e, pattern) };
        }
    }

    /** Run compile + match + highlight + replace and write the result into the
     *  store. Reads the current store values at call time so it works from the
     *  debounced effect. The test string is CAPPED to MAX_LIVE_MATCH_CHARS for
     *  the live match/highlight pass to avoid hanging on a pathological pattern;
     *  the replace preview also runs against the capped slice when truncated. */
    function compute(): void {
        const pattern = get(regexPattern);
        const flags = get(regexFlags);
        const fullTestString = get(regexTestString);
        const replacement = get(regexReplacement);

        const { re, error } = compileRegex(pattern, flags);
        if (!re) {
            regexComputed.set({ error, matches: [], segments: [], replaced: null, truncated: false });
            return;
        }

        const truncated = fullTestString.length > MAX_LIVE_MATCH_CHARS;
        const testString = truncated ? fullTestString.slice(0, MAX_LIVE_MATCH_CHARS) : fullTestString;

        // Matches.
        const matches: Match[] = [];
        if (testString) {
            const gre = new RegExp(re.source, re.flags.includes('g') ? re.flags : re.flags + 'g');
            let m: RegExpExecArray | null;
            while ((m = gre.exec(testString)) !== null) {
                matches.push({ text: m[0], index: m.index, groups: m.slice(1), named: m.groups ?? {} });
                if (m[0].length === 0) gre.lastIndex++; // avoid infinite loop on empty matches
                if (!flags.g) break; // non-global: only the first match
                if (matches.length > 10000) break; // safety cap
            }
        }

        // Highlight segments over the (possibly capped) test string.
        const segments: { text: string; match: boolean; idx?: number }[] = [];
        if (matches.length === 0) {
            segments.push({ text: testString, match: false });
        } else {
            let cursor = 0;
            for (let i = 0; i < matches.length; i++) {
                const mm = matches[i];
                if (mm.index > cursor) segments.push({ text: testString.slice(cursor, mm.index), match: false });
                segments.push({ text: mm.text, match: true, idx: i });
                cursor = mm.index + mm.text.length;
            }
            if (cursor < testString.length) segments.push({ text: testString.slice(cursor), match: false });
        }

        // Find/replace preview — honors the live flags (g = replace all). Runs
        // against the capped slice so it can't reintroduce the freeze.
        let replaced: string | null;
        try {
            replaced = testString.replace(re, replacement);
        } catch {
            replaced = null;
        }

        regexComputed.set({ error: null, matches, segments, replaced, truncated });
    }

    // Debounced compute: recomputes ~200ms after the pattern / flags / test
    // string / replacement settle, instead of synchronously on every keystroke.
    let debounceTimer: ReturnType<typeof setTimeout>;
    $effect(() => {
        // Track every input that affects the result so the effect re-runs.
        $regexPattern;
        $regexFlags;
        $regexTestString;
        $regexReplacement;
        clearTimeout(debounceTimer);
        debounceTimer = setTimeout(compute, REGEX_DEBOUNCE_MS);
        return () => clearTimeout(debounceTimer);
    });

    // Convenience reads of the computed result for the template.
    let matches = $derived($regexComputed.matches);
    let highlighted = $derived($regexComputed.segments);
    let replaced = $derived($regexComputed.replaced);

    // Plain-English token breakdown of the current pattern (never throws — pure
    // token parse, no regex execution, so it stays a cheap live derived).
    let breakdown = $derived($regexPattern ? explainPattern($regexPattern) : []);
    // Token color cycling: each type maps to a scoped class whose color is
    // sourced from semantic tokens / color-mix (defined in the style block below).
    const TOKEN_CLASS: Record<RegexTokenType, string> = {
        anchor: 'rx-tok rx-tok-anchor',
        class: 'rx-tok rx-tok-class',
        quantifier: 'rx-tok rx-tok-quantifier',
        group: 'rx-tok rx-tok-group',
        alternation: 'rx-tok rx-tok-alternation',
        escape: 'rx-tok rx-tok-escape',
        backref: 'rx-tok rx-tok-backref',
        literal: 'rx-tok rx-tok-literal',
    };

    // Builder blocks — clickable building blocks that append to the pattern
    type Block = { label: string; insert: string; description: string };
    const blocks: { category: string; items: Block[] }[] = [
        {
            category: 'Characters',
            items: [
                { label: '.', insert: '.', description: 'Any character (except newline)' },
                { label: '\\d', insert: '\\d', description: 'Any digit (0-9)' },
                { label: '\\D', insert: '\\D', description: 'Non-digit' },
                { label: '\\w', insert: '\\w', description: 'Word character (a-z, A-Z, 0-9, _)' },
                { label: '\\W', insert: '\\W', description: 'Non-word character' },
                { label: '\\s', insert: '\\s', description: 'Whitespace' },
                { label: '\\S', insert: '\\S', description: 'Non-whitespace' },
            ],
        },
        {
            category: 'Anchors',
            items: [
                { label: '^', insert: '^', description: 'Start of string/line' },
                { label: '$', insert: '$', description: 'End of string/line' },
                { label: '\\b', insert: '\\b', description: 'Word boundary' },
                { label: '\\B', insert: '\\B', description: 'Non-word boundary' },
            ],
        },
        {
            category: 'Quantifiers',
            items: [
                { label: '*', insert: '*', description: '0 or more' },
                { label: '+', insert: '+', description: '1 or more' },
                { label: '?', insert: '?', description: '0 or 1' },
                { label: '{n}', insert: '{2}', description: 'Exactly n times' },
                { label: '{n,m}', insert: '{2,5}', description: 'Between n and m times' },
                { label: '*?', insert: '*?', description: 'Lazy 0 or more' },
            ],
        },
        {
            category: 'Groups',
            items: [
                { label: '(...)', insert: '(...)', description: 'Capturing group' },
                { label: '(?:...)', insert: '(?:...)', description: 'Non-capturing group' },
                { label: '(?<name>...)', insert: '(?<name>...)', description: 'Named group' },
                { label: '|', insert: '|', description: 'Alternation' },
            ],
        },
        {
            category: 'Sets',
            items: [
                { label: '[abc]', insert: '[abc]', description: 'Any of a, b, or c' },
                { label: '[^abc]', insert: '[^abc]', description: 'None of a, b, or c' },
                { label: '[a-z]', insert: '[a-z]', description: 'Range a to z' },
            ],
        },
        {
            category: 'Lookaround',
            items: [
                { label: '(?=...)', insert: '(?=...)', description: 'Positive lookahead' },
                { label: '(?!...)', insert: '(?!...)', description: 'Negative lookahead' },
                { label: '(?<=...)', insert: '(?<=...)', description: 'Positive lookbehind' },
                { label: '(?<!...)', insert: '(?<!...)', description: 'Negative lookbehind' },
            ],
        },
    ];

    type Preset = { name: string; pattern: string; description: string };
    const presets: Preset[] = [
        { name: 'Email', pattern: '[\\w.+-]+@[\\w-]+\\.[\\w.-]+', description: 'Match email addresses' },
        { name: 'URL', pattern: 'https?:\\/\\/[\\w.-]+(?:\\/[\\w./?=&%-]*)?', description: 'HTTP(S) URL' },
        { name: 'IPv4', pattern: '\\b(?:\\d{1,3}\\.){3}\\d{1,3}\\b', description: 'IPv4 address' },
        { name: 'UUID', pattern: '\\b[\\da-f]{8}-[\\da-f]{4}-[\\da-f]{4}-[\\da-f]{4}-[\\da-f]{12}\\b', description: 'UUID format' },
        { name: 'ISO date', pattern: '\\b\\d{4}-\\d{2}-\\d{2}\\b', description: 'YYYY-MM-DD' },
        { name: 'Phone (intl)', pattern: '\\+?\\d{1,3}[\\s-]?\\(?\\d{1,4}\\)?[\\s-]?\\d{1,4}[\\s-]?\\d{1,9}', description: 'International phone' },
        { name: 'Hex color', pattern: '#(?:[\\da-fA-F]{3}){1,2}\\b', description: '#RRGGBB or #RGB' },
        { name: 'Slug', pattern: '^[a-z0-9]+(?:-[a-z0-9]+)*$', description: 'URL slug' },
        { name: 'Strong password', pattern: '^(?=.*[a-z])(?=.*[A-Z])(?=.*\\d)(?=.*[!@#$%^&*])[A-Za-z\\d!@#$%^&*]{8,}$', description: '8+ chars, mixed case, digit, symbol' },
    ];

    let patternInput: HTMLInputElement | null = $state(null);

    function appendToPattern(text: string) {
        const el = patternInput;
        const current = get(regexPattern);
        if (!el) {
            regexPattern.set(current + text);
            return;
        }
        const start = el.selectionStart ?? current.length;
        const end = el.selectionEnd ?? current.length;
        regexPattern.set(current.slice(0, start) + text + current.slice(end));
        requestAnimationFrame(() => {
            el.focus();
            const newPos = start + text.length;
            el.setSelectionRange(newPos, newPos);
        });
    }

    function flagString(): string {
        return Object.entries(get(regexFlags)).filter(([, v]) => v).map(([k]) => k).join('');
    }
    async function copyPattern() {
        const pattern = get(regexPattern);
        if (!pattern) return;
        await navigator.clipboard.writeText(`/${pattern}/${flagString()}`);
        toast('Copied regex literal', 'success');
    }
    async function copyJsCode() {
        const pattern = get(regexPattern);
        if (!pattern) return;
        await navigator.clipboard.writeText(`const re = /${pattern}/${flagString()};`);
        toast('Copied JS code', 'success');
    }

    // ── Suggest from examples (grex backend) ────────────────────────────
    // `exampleInput` is lifted into the store; these three are ephemeral
    // suggestion options that don't need to survive navigation.
    let suggestBusy = $state(false);
    let suggestConvertClasses = $state(true);
    let suggestCaseInsensitive = $state(false);

    async function suggestPatternFromExamples() {
        if (suggestBusy) return;
        const examples = get(regexExampleInput).split('\n').map((s) => s.trim()).filter((s) => s.length > 0);
        if (examples.length === 0) {
            toast('Add at least one example first', 'info');
            return;
        }
        suggestBusy = true;
        try {
            // Case-insensitivity is applied as the `i` flag below — NOT baked into
            // the pattern (grex's CI emits an inline (?i) the JS engine rejects).
            const result = await invoke<{ pattern: string; examplesUsed: number }>('regex_from_examples', {
                options: { examples, convertClasses: suggestConvertClasses },
            });
            regexPattern.set(result.pattern);
            regexFlags.update((f) => ({ ...f, i: suggestCaseInsensitive }));
            toast(`Built a pattern from ${result.examplesUsed} example${result.examplesUsed === 1 ? '' : 's'}`, 'success');
        } catch (error) {
            errorToast("Couldn't suggest a pattern from these examples", error, {
                hint: 'Try simpler examples first (one per line) — the suggester needs a few similar samples to find a pattern.',
            });
        } finally {
            suggestBusy = false;
        }
    }

    const flagDescription = (k: string): string =>
        k === 'g' ? 'global' : k === 'i' ? 'case-insensitive' : k === 'm' ? 'multiline' : k === 's' ? 'dotAll' : 'unicode';
</script>

<div class="dt-panel">
    <div class="dt-head">
        <div class="dt-head-text">
            <h2 class="dt-title">Regex Tool — build, test &amp; explain patterns</h2>
            <p class="dt-desc">Write a regular expression, test it against sample text with live match highlighting, and get a plain-English breakdown. Everything runs locally.</p>
        </div>
    </div>

    <!-- Mode toolbar -->
    <div class="dt-toolbar">
        <div class="dt-seg" role="tablist" aria-label="Regex mode">
            <button
                type="button"
                role="tab"
                aria-selected={$regexTab === 'build'}
                class="dt-seg-btn"
                class:is-active={$regexTab === 'build'}
                onclick={() => ($regexTab = 'build')}
            >
                Builder
            </button>
            <button
                type="button"
                role="tab"
                aria-selected={$regexTab === 'test'}
                class="dt-seg-btn"
                class:is-active={$regexTab === 'test'}
                onclick={() => ($regexTab = 'test')}
            >
                Tester
            </button>
        </div>
        <div class="dt-toolbar-end">
            <Button variant="ghost" icon={Copy} onclick={copyPattern}>/pattern/</Button>
            <Button variant="primary" icon={Copy} onclick={copyJsCode}>Copy JS code</Button>
        </div>
    </div>

    <!-- Pattern input -->
    <ToolPanel padding="md">
        <div class="dt-io">
            <div class="dt-io-head">
                <span class="dt-section-label">Pattern</span>
            </div>
            <div class="rx-pattern">
                <span class="dt-text-muted dt-mono">/</span>
                <input
                    id="re-pattern"
                    bind:this={patternInput}
                    bind:value={$regexPattern}
                    spellcheck="false"
                    class="dt-input dt-mono rx-pattern-input"
                    aria-label="Regular expression pattern"
                />
                <span class="dt-text-muted dt-mono">/</span>
                <input
                    type="text"
                    value={flagString()}
                    readonly
                    class="dt-input dt-mono rx-pattern-flags"
                    aria-label="Active flags"
                />
            </div>
        </div>

        <!-- Flags -->
        <div class="rx-flags">
            {#each Object.entries($regexFlags) as [k, v]}
                <label class="rx-flag">
                    <Checkbox
                        checked={v}
                        ariaLabel={`Flag ${k} (${flagDescription(k)})`}
                        onchange={(checked) => regexFlags.update((f) => ({ ...f, [k]: checked }))}
                    />
                    <span class="dt-mono">{k}</span>
                    <span class="dt-text-muted">{flagDescription(k)}</span>
                </label>
            {/each}
        </div>

        {#if $regexComputed.error}
            <div class="dt-error rx-block-top">{$regexComputed.error}</div>
        {/if}

        <!-- Annotated breakdown -->
        {#if breakdown.length}
            <div class="rx-breakdown">
                <div class="dt-section-label">
                    Breakdown <span class="rx-breakdown-hint">— hover a token for what it does</span>
                </div>
                <div class="rx-tokens">
                    {#each breakdown as t}
                        <span class={TOKEN_CLASS[t.type]} title={t.label}>{t.text}</span>
                    {/each}
                </div>
            </div>
        {/if}
    </ToolPanel>

    {#if $regexTab === 'build'}
        <!-- BUILDER -->
        <ToolPanel padding="md">
            <div class="dt-col">
                <div class="dt-io">
                    <span class="dt-section-label">Click to insert at cursor</span>
                    <div class="rx-blocks">
                        {#each blocks as group}
                            <div class="rx-block-group">
                                <div class="rx-block-cat">{group.category}</div>
                                <div class="rx-block-items">
                                    {#each group.items as b}
                                        <button
                                            type="button"
                                            onclick={() => appendToPattern(b.insert)}
                                            title={b.description}
                                            class="rx-block-btn dt-mono"
                                        >
                                            {b.label}
                                        </button>
                                    {/each}
                                </div>
                            </div>
                        {/each}
                    </div>
                </div>

                <!-- Suggest from examples (grex-powered) -->
                <ToolPanel padding="sm" tone="panel-2">
                    <div class="rx-suggest-head">
                        <Sparkles class="rx-suggest-ico" />
                        <span class="dt-section-label">Suggest from examples</span>
                    </div>
                    <p class="dt-hint rx-suggest-copy">Paste a few strings you want the pattern to match (one per line). KeepItLocal builds a regex that matches all of them.</p>
                    <textarea
                        bind:value={$regexExampleInput}
                        spellcheck="false"
                        placeholder={'2024-01-15\n2024-02-28\n2023-12-31'}
                        class="dt-ta rx-suggest-ta"
                        aria-label="Example strings, one per line"
                    ></textarea>
                    <div class="rx-suggest-opts">
                        <label class="rx-flag">
                            <Checkbox bind:checked={suggestConvertClasses} ariaLabel="Compact character classes" />
                            <span class="dt-text-secondary">Compact ( \d / \w / \s, with repeats )</span>
                        </label>
                        <label class="rx-flag">
                            <Checkbox bind:checked={suggestCaseInsensitive} ariaLabel="Case-insensitive" />
                            <span class="dt-text-secondary">Case-insensitive</span>
                        </label>
                    </div>
                    <Button
                        variant="primary"
                        size="sm"
                        icon={Sparkles}
                        loading={suggestBusy}
                        disabled={suggestBusy}
                        onclick={() => void suggestPatternFromExamples()}
                    >
                        {suggestBusy ? 'Building…' : 'Suggest pattern'}
                    </Button>
                </ToolPanel>

                <div class="dt-io">
                    <span class="dt-section-label">Common patterns</span>
                    <div class="rx-presets">
                        {#each presets as p}
                            <button type="button" onclick={() => regexPattern.set(p.pattern)} class="rx-preset">
                                <div class="rx-preset-name">{p.name}</div>
                                <div class="rx-preset-desc">{p.description}</div>
                                <code class="rx-preset-code dt-mono">{p.pattern}</code>
                            </button>
                        {/each}
                    </div>
                </div>

                <div class="dt-hint">
                    Switch to the <button type="button" onclick={() => ($regexTab = 'test')} class="rx-inline-link">Tester</button> tab to test your pattern against sample text.
                </div>
            </div>
        </ToolPanel>
    {:else}
        <!-- TESTER -->
        <ToolPanel padding="md">
            <div class="dt-two">
                <div class="dt-io">
                    <div class="dt-io-head">
                        <span class="dt-section-label">Test string</span>
                        <span class="dt-text-muted rx-meta">{$regexTestString.length} chars</span>
                    </div>
                    <textarea
                        id="re-input"
                        bind:value={$regexTestString}
                        spellcheck="false"
                        class="dt-ta is-tall"
                        aria-label="Test string"
                    ></textarea>
                    {#if $regexComputed.truncated}
                        <!-- FREEZE GUARD: live matching is capped so a
                             catastrophic-backtracking pattern can't hang the UI
                             on a huge string. -->
                        <div class="dt-hint rx-truncate-note">
                            Live matching limited to the first {(MAX_LIVE_MATCH_CHARS / 1000).toFixed(0)}k characters for performance.
                        </div>
                    {/if}
                </div>

                <div class="dt-io">
                    <div class="dt-io-head">
                        <span class="dt-section-label">Matches ({matches.length})</span>
                    </div>
                    <div class="rx-results">
                        <div class="rx-highlight dt-mono">
                            {#each highlighted as seg}
                                {#if seg.match}
                                    <mark class="rx-mark">{seg.text}</mark>
                                {:else}
                                    <span>{seg.text}</span>
                                {/if}
                            {/each}
                        </div>
                        {#if matches.length > 0}
                            <div class="rx-match-list">
                                {#each matches as m, i}
                                    <div class="rx-match">
                                        <div class="rx-match-head">
                                            <span class="dt-text-muted rx-match-no">#{i + 1}</span>
                                            <span class="dt-mono rx-match-text">{m.text}</span>
                                            <span class="dt-text-muted rx-match-at">at {m.index}</span>
                                        </div>
                                        {#if m.groups.length > 0}
                                            <div class="rx-match-groups">
                                                {#each m.groups as g, gi}
                                                    <div class="dt-kv"><span class="dt-kv-key dt-text-muted">${gi + 1}</span><span class="dt-kv-val dt-mono">{g ?? ''}</span></div>
                                                {/each}
                                            </div>
                                        {/if}
                                        {#if Object.keys(m.named).length > 0}
                                            <div class="rx-match-groups">
                                                {#each Object.entries(m.named) as [name, val]}
                                                    <div class="dt-kv"><span class="dt-kv-key dt-text-muted">{name}</span><span class="dt-kv-val dt-mono">{val}</span></div>
                                                {/each}
                                            </div>
                                        {/if}
                                    </div>
                                {/each}
                            </div>
                        {:else}
                            <div class="rx-empty">
                                <EmptyState icon={RegexIcon} variant="compact" title="No matches" description="Adjust your pattern or flags to match the test string." />
                            </div>
                        {/if}
                    </div>
                </div>
            </div>

            <!-- Find / replace preview -->
            <ToolPanel padding="sm" tone="panel-2">
                <div class="dt-io">
                    <div class="dt-io-head">
                        <span class="dt-section-label">Replace with</span>
                        <span class="dt-text-muted rx-replace-hint">$1 · $&lt;name&gt; · $&amp; whole match · enable <span class="dt-mono">g</span> to replace all</span>
                    </div>
                    <input
                        id="re-replace"
                        bind:value={$regexReplacement}
                        spellcheck="false"
                        placeholder="e.g. [redacted]  or  $1"
                        class="dt-input dt-mono"
                        aria-label="Replacement string"
                    />
                    <div class="dt-section-label rx-replace-label">Result preview</div>
                    <pre class="dt-pre rx-replace-out">{replaced ?? ''}</pre>
                </div>
            </ToolPanel>
        </ToolPanel>
    {/if}
</div>

<style>
    /* Pattern field row: pattern input plus flags box */
    .rx-pattern {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }
    .rx-pattern-input {
        flex: 1;
        min-width: 0;
    }
    .rx-pattern-flags {
        width: 64px;
        flex: none;
        color: var(--color-text-secondary);
    }

    /*Flag toggles */
    .rx-flags {
        display: flex;
        flex-wrap: wrap;
        gap: 8px 16px;
        margin-top: 12px;
    }
    .rx-flag {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-size: 12px;
        cursor: pointer;
    }

    /*Error / block spacing */
    .rx-block-top {
        margin-top: 12px;
    }

    /*Breakdown tokens */
    .rx-breakdown {
        display: flex;
        flex-direction: column;
        gap: 6px;
        margin-top: 14px;
    }
    .rx-breakdown-hint {
        text-transform: none;
        letter-spacing: normal;
        font-weight: 400;
        color: color-mix(in srgb, var(--color-muted) 70%, transparent);
    }
    .rx-tokens {
        display: flex;
        flex-wrap: wrap;
        gap: 4px;
    }
    .rx-tok {
        padding: 2px 6px;
        border-radius: 4px;
        border: 1px solid;
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
        font-size: 12px;
        white-space: pre;
    }
    /* Capture-group and token color cycling, sourced from semantic tokens
       via color-mix so it tracks the theme (no hardcoded hex). */
    .rx-tok-anchor {
        color: var(--color-error);
        background: color-mix(in srgb, var(--color-error) 12%, transparent);
        border-color: color-mix(in srgb, var(--color-error) 35%, transparent);
    }
    .rx-tok-class {
        color: var(--color-success);
        background: color-mix(in srgb, var(--color-success) 12%, transparent);
        border-color: color-mix(in srgb, var(--color-success) 35%, transparent);
    }
    .rx-tok-quantifier {
        color: var(--color-warning);
        background: color-mix(in srgb, var(--color-warning) 12%, transparent);
        border-color: color-mix(in srgb, var(--color-warning) 35%, transparent);
    }
    .rx-tok-group {
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 12%, transparent);
        border-color: color-mix(in srgb, var(--color-accent) 35%, transparent);
    }
    .rx-tok-alternation {
        color: color-mix(in srgb, var(--color-accent) 65%, var(--color-error));
        background: color-mix(in srgb, var(--color-accent) 9%, transparent);
        border-color: color-mix(in srgb, var(--color-accent) 30%, transparent);
    }
    .rx-tok-escape {
        color: color-mix(in srgb, var(--color-accent) 80%, var(--color-text));
        background: color-mix(in srgb, var(--color-accent) 9%, transparent);
        border-color: color-mix(in srgb, var(--color-accent) 28%, transparent);
    }
    .rx-tok-backref {
        color: color-mix(in srgb, var(--color-success) 70%, var(--color-accent));
        background: color-mix(in srgb, var(--color-success) 9%, transparent);
        border-color: color-mix(in srgb, var(--color-success) 30%, transparent);
    }
    .rx-tok-literal {
        color: var(--color-text-secondary);
        background: var(--color-panel-2);
        border-color: var(--color-border);
    }

    /*Builder blocks */
    .rx-blocks {
        display: grid;
        gap: 12px;
        grid-template-columns: 1fr;
    }
    @media (min-width: 1024px) {
        .rx-blocks {
            grid-template-columns: 1fr 1fr;
        }
    }
    .rx-block-group {
        padding: 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    .rx-block-cat {
        margin-bottom: 6px;
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.05em;
        text-transform: uppercase;
        color: var(--color-muted);
    }
    .rx-block-items {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
    }
    .rx-block-btn {
        padding: 4px 8px;
        font-size: 12px;
        color: var(--color-text);
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 6px;
        cursor: pointer;
        transition: border-color var(--dur-micro, 120ms) var(--ease-out, ease);
    }
    .rx-block-btn:hover {
        border-color: color-mix(in srgb, var(--color-accent) 70%, var(--color-border));
    }

    /*Suggest from examples */
    .rx-suggest-head {
        display: flex;
        align-items: center;
        gap: 8px;
        margin-bottom: 8px;
    }
    .rx-suggest-head :global(.rx-suggest-ico) {
        width: 16px;
        height: 16px;
        color: var(--color-accent);
    }
    .rx-suggest-copy {
        margin: 0 0 8px;
    }
    .rx-suggest-ta {
        min-height: 112px;
        margin-bottom: 8px;
    }
    .rx-suggest-opts {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px 16px;
        margin-bottom: 12px;
    }

    /*Presets */
    .rx-presets {
        display: grid;
        gap: 8px;
        grid-template-columns: 1fr;
    }
    @media (min-width: 640px) {
        .rx-presets {
            grid-template-columns: 1fr 1fr;
        }
    }
    @media (min-width: 1024px) {
        .rx-presets {
            grid-template-columns: 1fr 1fr 1fr;
        }
    }
    .rx-preset {
        padding: 12px;
        text-align: left;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        cursor: pointer;
        transition: border-color var(--dur-micro, 120ms) var(--ease-out, ease);
    }
    .rx-preset:hover {
        border-color: color-mix(in srgb, var(--color-accent) 70%, var(--color-border));
    }
    .rx-preset-name {
        font-size: 13px;
        font-weight: 500;
        color: var(--color-text);
    }
    .rx-preset-desc {
        margin-top: 2px;
        font-size: 12px;
        color: var(--color-muted);
    }
    .rx-preset-code {
        display: block;
        margin-top: 4px;
        font-size: 10px;
        color: var(--color-text-secondary);
        word-break: break-all;
    }

    /*Inline link button */
    .rx-inline-link {
        padding: 0;
        background: transparent;
        border: none;
        color: var(--color-accent);
        cursor: pointer;
    }
    .rx-inline-link:hover {
        text-decoration: underline;
    }

    /*Tester results */
    .rx-meta,
    .rx-replace-hint {
        font-size: 11px;
    }
    .rx-truncate-note {
        margin-top: 6px;
        color: var(--color-warning);
    }
    .rx-replace-hint {
        text-transform: none;
        letter-spacing: normal;
        font-weight: 400;
    }
    .rx-results {
        height: 56vh;
        overflow-y: auto;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }
    .rx-highlight {
        padding: 12px;
        font-size: 12px;
        line-height: 1.55;
        white-space: pre-wrap;
        word-break: break-word;
        border-bottom: 1px solid var(--color-border);
    }
    .rx-mark {
        /* Match highlight: accent tint sourced from tokens, not hex. */
        background: color-mix(in srgb, var(--color-accent) 28%, transparent);
        color: var(--color-text);
        border-radius: 3px;
        padding: 0 2px;
    }
    .rx-match-list {
        display: flex;
        flex-direction: column;
    }
    .rx-match {
        padding: 8px 12px;
        font-size: 12px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
    }
    .rx-match-head {
        display: flex;
        align-items: baseline;
        gap: 8px;
    }
    .rx-match-no {
        flex: none;
    }
    .rx-match-text {
        color: var(--color-accent);
        word-break: break-all;
    }
    .rx-match-at {
        flex: none;
        margin-left: auto;
    }
    .rx-match-groups {
        margin-top: 4px;
        margin-left: 24px;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
    .rx-empty {
        padding: 8px 4px;
    }

    /*Replace preview */
    .rx-replace-label {
        margin-top: 8px;
    }
    .rx-replace-out {
        margin-top: 6px;
        max-height: 12rem;
    }

    @media (prefers-reduced-motion: reduce) {
        .rx-block-btn,
        .rx-preset {
            transition: none;
        }
    }
</style>
