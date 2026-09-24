<script lang="ts">
    /*
      DevToolkit — one calm surface for the light developer utilities.

      2026-06-05: folds nine standalone Development tools into a single
      hub navigated by a sectioned left rail (the app's canonical
      selected-item pattern — panel-2 surface + accent pill strip +
      accent icon), grouped into Format & Inspect / Generate / Secure.

      The two heavy, stateful managers — SSH Key Manager and
      Encrypt / Decrypt — deliberately stay standalone tools.

      Each mode is a body-only panel (no nested ToolPage). Panels are
      lazy-mounted on first visit and kept alive (hidden) so in-progress
      work survives a mode switch. Shared widget styles live here as
      `:global(.dt-*)` so every panel composes from one source of truth.
    */
    import { Wrench, Key, Regex, Database, GitCompare, Fingerprint, Sparkles, Timer, ShieldAlert } from '@lucide/svelte';
    import { ToolPage } from '$lib/ui';
    import JwtPanel from './devkit/JwtPanel.svelte';
    import RegexPanel from './devkit/RegexPanel.svelte';
    import SqlPanel from './devkit/SqlPanel.svelte';
    import DiffPanel from './devkit/DiffPanel.svelte';
    // NOTE (2026-07-23): the Markdown panel moved OUT of Developer Tools and
    // became the Documents-pack "Markdown Converter" tool, which absorbed it
    // wholesale and added HTML/PDF/Word/text file export. A writer converting
    // notes into a Word file was never going to look under Developer Tools.
    // `devkit/MarkdownPanel.svelte` is kept on disk as the backup copy; it is
    // no longer imported. Its state stores still live in `devkitPanels.ts`.
    // import MarkdownPanel from './devkit/MarkdownPanel.svelte';
    import IdGenPanel from './devkit/IdGenPanel.svelte';
    import FakeDataPanel from './devkit/FakeDataPanel.svelte';
    import CronPanel from './devkit/CronPanel.svelte';
    import SecretScanPanel from './devkit/SecretScanPanel.svelte';
    import { get } from 'svelte/store';
    import { devkitMode, type DevkitMode } from '$lib/stores/devkitPanels';

    type ModeId = DevkitMode;

    type Mode = { id: ModeId; label: string; sub: string; icon: typeof Key };

    const sections: { label: string; items: Mode[] }[] = [
        {
            label: 'Format & Inspect',
            items: [
                { id: 'jwt', label: 'JWT Decoder', sub: 'Decode & verify tokens', icon: Key },
                { id: 'regex', label: 'Regex Tool', sub: 'Build, test & explain', icon: Regex },
                { id: 'sql', label: 'SQL Formatter', sub: 'Format & lint queries', icon: Database },
                { id: 'diff', label: 'Diff Viewer', sub: 'Compare text', icon: GitCompare },
                // Markdown moved to Documents → "Markdown Converter" (2026-07-23).
                // To restore, also re-add `FileCode` to the @lucide/svelte import
                // above — it was dropped because this was its only use.
                // { id: 'markdown', label: 'Markdown', sub: 'Preview & HTML → MD', icon: FileCode },
            ],
        },
        {
            label: 'Generate',
            items: [
                { id: 'idgen', label: 'ID Generator', sub: 'UUID · ULID · NanoID', icon: Fingerprint },
                { id: 'fakedata', label: 'Fake Data', sub: 'Test fixtures', icon: Sparkles },
                { id: 'cron', label: 'Cron Builder', sub: 'Build & explain cron', icon: Timer },
            ],
        },
        {
            label: 'Secure',
            items: [
                { id: 'secretscan', label: 'Secret Scanner', sub: 'Find leaked credentials', icon: ShieldAlert },
            ],
        },
    ];

    const restoredMode = get(devkitMode);
    let mode = $state<ModeId>(restoredMode);
    let visited = $state<Record<ModeId, boolean>>({
        jwt: restoredMode === 'jwt',
        regex: restoredMode === 'regex',
        sql: restoredMode === 'sql',
        diff: restoredMode === 'diff',
        idgen: restoredMode === 'idgen',
        fakedata: restoredMode === 'fakedata',
        cron: restoredMode === 'cron',
        secretscan: restoredMode === 'secretscan',
    });

    function select(id: ModeId): void {
        mode = id;
        devkitMode.set(id);
        if (!visited[id]) visited = { ...visited, [id]: true };
    }
</script>

<ToolPage
    icon={Wrench}
    iconTint="#22c55e"
    title="Developer Tools"
    description=""
    width="wide"
    fill={false}
>
    <div class="dt-shell">
        <nav class="dt-rail" aria-label="Developer tools">
            {#each sections as section (section.label)}
                <div class="dt-rail-group">
                    <div class="dt-rail-heading">{section.label}</div>
                    {#each section.items as m (m.id)}
                        <button
                            type="button"
                            class="dt-rail-btn"
                            class:is-active={mode === m.id}
                            aria-current={mode === m.id ? 'page' : undefined}
                            onclick={() => select(m.id)}
                        >
                            <m.icon class="dt-rail-ico" />
                            <span class="dt-rail-text">
                                <span class="dt-rail-label">{m.label}</span>
                                <span class="dt-rail-sub">{m.sub}</span>
                            </span>
                        </button>
                    {/each}
                </div>
            {/each}
        </nav>

        <main class="dt-main">
            {#if visited.jwt}<div class="dt-pane" class:hidden={mode !== 'jwt'}><JwtPanel /></div>{/if}
            {#if visited.regex}<div class="dt-pane" class:hidden={mode !== 'regex'}><RegexPanel /></div>{/if}
            {#if visited.sql}<div class="dt-pane" class:hidden={mode !== 'sql'}><SqlPanel /></div>{/if}
            {#if visited.diff}<div class="dt-pane" class:hidden={mode !== 'diff'}><DiffPanel /></div>{/if}
            <!-- Markdown pane removed 2026-07-23 — see the import note at the top.
                 {#if visited.markdown}<div class="dt-pane" class:hidden={mode !== 'markdown'}><MarkdownPanel /></div>{/if} -->
            {#if visited.idgen}<div class="dt-pane" class:hidden={mode !== 'idgen'}><IdGenPanel /></div>{/if}
            {#if visited.fakedata}<div class="dt-pane" class:hidden={mode !== 'fakedata'}><FakeDataPanel /></div>{/if}
            {#if visited.cron}<div class="dt-pane" class:hidden={mode !== 'cron'}><CronPanel /></div>{/if}
            {#if visited.secretscan}<div class="dt-pane" class:hidden={mode !== 'secretscan'}><SecretScanPanel /></div>{/if}
        </main>
    </div>
</ToolPage>

<style>
    /* ── Shell layout: left rail + main ── */
    .dt-shell {
        display: grid;
        grid-template-columns: 1fr;
        gap: 16px;
        align-items: start;
    }
    @media (min-width: 1024px) {
        .dt-shell {
            grid-template-columns: 216px minmax(0, 1fr);
        }
    }

    /* ── Sectioned mode rail (canonical selected pattern) ── */
    .dt-rail {
        display: flex;
        flex-direction: column;
        gap: 12px;
        padding: 8px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
    }
    .dt-rail-group {
        display: flex;
        flex-direction: column;
        gap: 3px;
    }
    .dt-rail-heading {
        padding: 2px 10px 4px;
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.05em;
        text-transform: uppercase;
        color: var(--color-muted);
    }
    .dt-rail-btn {
        position: relative;
        display: flex;
        align-items: center;
        gap: 10px;
        width: 100%;
        padding: 7px 10px 7px 14px;
        border: none;
        border-radius: var(--radius-control, 8px);
        background: transparent;
        color: var(--color-text-secondary);
        text-align: left;
        cursor: pointer;
        transition: background-color var(--dur-micro, 120ms) var(--ease-out, ease);
    }
    .dt-rail-btn:hover {
        background: color-mix(in srgb, var(--color-text) 6%, transparent);
        color: var(--color-text);
    }
    .dt-rail-btn:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: -2px;
    }
    .dt-rail-btn.is-active {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .dt-rail-btn.is-active::before {
        content: '';
        position: absolute;
        left: 4px;
        top: 7px;
        bottom: 7px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .dt-rail-btn :global(.dt-rail-ico) {
        flex: none;
        width: 16px;
        height: 16px;
        color: var(--color-muted);
    }
    .dt-rail-btn.is-active :global(.dt-rail-ico) {
        color: var(--color-accent);
    }
    .dt-rail-text {
        display: flex;
        flex-direction: column;
        min-width: 0;
        line-height: 1.25;
    }
    .dt-rail-label {
        font-size: 12.5px;
        font-weight: 500;
    }
    .dt-rail-sub {
        font-size: 10px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .dt-main {
        min-width: 0;
    }
    .dt-pane {
        min-width: 0;
    }

    /* ════════════════════════════════════════════════════════════════
       Shared panel widgets — used by every devkit/ panel via :global.
       Semantic tokens only.
       ════════════════════════════════════════════════════════════════ */

    /* Scaffold */
    :global(.dt-panel) {
        display: flex;
        flex-direction: column;
        gap: 12px;
        min-width: 0;
    }
    :global(.dt-head) {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 12px;
        flex-wrap: wrap;
    }
    :global(.dt-head-text) {
        min-width: 0;
    }
    :global(.dt-title) {
        margin: 0;
        font-size: 15px;
        font-weight: 600;
        color: var(--color-text);
        letter-spacing: -0.01em;
    }
    :global(.dt-desc) {
        margin: 2px 0 0;
        font-size: 12px;
        line-height: 1.45;
        color: var(--color-muted);
        max-width: 74ch;
    }

    /* Toolbar */
    :global(.dt-toolbar) {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
    }
    :global(.dt-toolbar-end) {
        margin-left: auto;
        display: inline-flex;
        align-items: center;
        gap: 8px;
    }

    /* Grids */
    :global(.dt-grid) {
        display: grid;
        grid-template-columns: 1fr;
        gap: 12px;
        align-items: start;
    }
    @media (min-width: 1180px) {
        :global(.dt-grid) {
            grid-template-columns: minmax(0, 1fr) 300px;
        }
    }
    :global(.dt-two) {
        display: grid;
        grid-template-columns: 1fr;
        gap: 12px;
        align-items: start;
    }
    @media (min-width: 980px) {
        :global(.dt-two) {
            grid-template-columns: 1fr 1fr;
        }
    }
    :global(.dt-col) {
        display: flex;
        flex-direction: column;
        gap: 12px;
        min-width: 0;
    }

    /* Fields */
    :global(.dt-fields) {
        display: grid;
        gap: 10px;
    }
    :global(.dt-fields.cols-2) {
        grid-template-columns: 1fr 1fr;
    }
    :global(.dt-fields.cols-3) {
        grid-template-columns: 1fr 1fr 1fr;
    }
    :global(.dt-field) {
        display: flex;
        flex-direction: column;
        gap: 5px;
        min-width: 0;
    }
    :global(.dt-label) {
        font-size: 11.5px;
        color: var(--color-text-secondary);
    }
    :global(.dt-input),
    :global(.dt-select) {
        width: 100%;
        height: 32px;
        padding: 0 8px;
        font-size: 13px;
        color: var(--color-text);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }
    :global(.dt-select) {
        cursor: pointer;
    }
    :global(.dt-select option) {
        background: var(--color-panel);
        color: var(--color-text);
    }
    :global(.dt-input:focus-visible),
    :global(.dt-select:focus-visible) {
        outline: 2px solid var(--color-accent);
        outline-offset: 1px;
    }
    :global(.dt-input:disabled),
    :global(.dt-select:disabled) {
        opacity: 0.55;
    }
    :global(.dt-checks) {
        display: flex;
        flex-direction: column;
        gap: 9px;
    }
    :global(.dt-hint) {
        font-size: 11px;
        line-height: 1.5;
        color: var(--color-muted);
    }
    :global(.dt-mono) {
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
    }

    /* Text editors / output */
    :global(.dt-io) {
        display: flex;
        flex-direction: column;
        gap: 6px;
        min-width: 0;
    }
    :global(.dt-io-head) {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
    }
    :global(.dt-ta) {
        width: 100%;
        min-height: 220px;
        padding: 10px 12px;
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
        font-size: 12px;
        line-height: 1.55;
        color: var(--color-text);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        resize: vertical;
    }
    :global(.dt-ta.is-tall) {
        min-height: 56vh;
    }
    :global(.dt-ta:focus-visible) {
        outline: 2px solid var(--color-accent);
        outline-offset: 1px;
    }
    :global(.dt-ta:disabled),
    :global(.dt-ta[readonly]) {
        color: var(--color-text);
    }
    :global(.dt-ta.is-error) {
        border-color: color-mix(in srgb, var(--color-error) 50%, var(--color-border));
        color: var(--color-error);
    }
    :global(.dt-pre) {
        margin: 0;
        padding: 10px 12px;
        max-height: 56vh;
        overflow: auto;
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
        font-size: 12px;
        line-height: 1.55;
        color: var(--color-text);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        white-space: pre-wrap;
        word-break: break-word;
    }

    /* Key/value (claims, stats) */
    :global(.dt-kv) {
        display: flex;
        align-items: baseline;
        gap: 8px;
        font-size: 12px;
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
    }
    :global(.dt-kv-key) {
        flex: none;
        color: var(--color-text-secondary);
    }
    :global(.dt-kv-val) {
        min-width: 0;
        color: var(--color-text);
        word-break: break-word;
    }
    :global(.dt-kv-note) {
        margin-left: auto;
        flex: none;
        font-size: 10px;
        color: var(--color-muted);
    }
    :global(.dt-kv-note.is-warn) {
        color: var(--color-error);
    }

    /* Segmented control */
    :global(.dt-seg) {
        display: inline-flex;
        /* Wrap instead of overflowing: `inline-flex` still hugs its content
           when the pills fit, but max-width pins it to the container and
           flex-wrap moves the overflow onto a second row. Without these, a
           long option set (e.g. IdGen's five formats, ending in "NanoID
           custom") rendered straight out past the panel's right edge. */
        flex-wrap: wrap;
        max-width: 100%;
        padding: 3px;
        gap: 3px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel);
    }
    :global(.dt-seg-btn) {
        position: relative;
        display: inline-flex;
        align-items: center;
        gap: 6px;
        /* Keep each label on one line — otherwise a two-word option like
           "UUID v1-like" breaks inside its own pill and the row grows. */
        white-space: nowrap;
        padding: 5px 14px 5px 18px;
        border: none;
        border-radius: calc(var(--radius-control, 8px) - 2px);
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12.5px;
        font-weight: 500;
        cursor: pointer;
        transition: color var(--dur-micro, 120ms) var(--ease-out, ease);
    }
    :global(.dt-seg-btn:hover) {
        color: var(--color-text);
    }
    :global(.dt-seg-btn.is-active) {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    :global(.dt-seg-btn.is-active)::before {
        position: absolute;
        top: 6px;
        bottom: 6px;
        left: 5px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }
    :global(.dt-seg-btn:focus-visible) {
        outline: 2px solid var(--color-accent);
        outline-offset: 2px;
    }

    /* Lists / rows */
    :global(.dt-list) {
        display: flex;
        flex-direction: column;
        max-height: 22rem;
        overflow: auto;
        scrollbar-gutter: stable;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    :global(.dt-row) {
        position: relative;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        width: 100%;
        padding: 7px 10px 7px 12px;
        font-size: 12px;
        text-align: left;
        background: transparent;
        border: none;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
        color: var(--color-text);
    }
    :global(.dt-row:first-child) {
        border-top: none;
    }
    :global(button.dt-row) {
        cursor: pointer;
    }
    :global(button.dt-row:hover) {
        background: color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    :global(.dt-row.is-selected) {
        background: var(--color-panel-2);
    }
    :global(.dt-row.is-selected)::before {
        content: '';
        position: absolute;
        left: 2px;
        top: 7px;
        bottom: 7px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    :global(.dt-row-main) {
        min-width: 0;
        flex: 1;
    }
    :global(.dt-row-name) {
        display: block;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--color-text);
    }
    :global(.dt-row-sub) {
        display: block;
        font-size: 10.5px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    :global(.dt-link-btn) {
        flex: none;
        padding: 2px 4px;
        font-size: 11.5px;
        color: var(--color-muted);
        background: transparent;
        border: none;
        cursor: pointer;
    }
    :global(.dt-link-btn:hover:not(:disabled)) {
        color: var(--color-text);
    }
    :global(.dt-link-btn:disabled) {
        opacity: 0.5;
        cursor: default;
    }

    /* Stat cards */
    :global(.dt-stats) {
        display: grid;
        gap: 8px;
        grid-template-columns: repeat(2, 1fr);
    }
    @media (min-width: 900px) {
        :global(.dt-stats.cols-4) {
            grid-template-columns: repeat(4, 1fr);
        }
        :global(.dt-stats.cols-3) {
            grid-template-columns: repeat(3, 1fr);
        }
    }
    :global(.dt-stat) {
        padding: 9px 11px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    :global(.dt-stat-label) {
        font-size: 10.5px;
        color: var(--color-muted);
    }
    :global(.dt-stat-val) {
        margin-top: 2px;
        font-size: 17px;
        font-weight: 600;
        color: var(--color-text);
        font-variant-numeric: tabular-nums;
    }
    :global(.dt-stat-val.is-accent) {
        color: var(--color-accent);
    }
    :global(.dt-stat-val.is-success) {
        color: var(--color-success);
    }
    :global(.dt-stat-val.is-error) {
        color: var(--color-error);
    }

    /* Result / finding rows */
    :global(.dt-results) {
        display: flex;
        flex-direction: column;
        gap: 6px;
        max-height: 24rem;
        overflow: auto;
        scrollbar-gutter: stable;
    }
    :global(.dt-result) {
        padding: 9px 11px;
        font-size: 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    :global(.dt-result.is-fail) {
        border-color: color-mix(in srgb, var(--color-error) 45%, var(--color-border));
        background: color-mix(in srgb, var(--color-error) 9%, transparent);
    }
    :global(.dt-result-row) {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 10px;
    }
    :global(.dt-result-name) {
        min-width: 0;
        flex: 1;
        color: var(--color-text);
        word-break: break-word;
    }
    :global(.dt-result-detail) {
        margin-top: 3px;
        font-size: 10.5px;
        color: var(--color-muted);
    }
    :global(.dt-badge) {
        flex: none;
        padding: 1px 7px;
        border-radius: 6px;
        font-size: 9.5px;
        font-weight: 700;
        letter-spacing: 0.03em;
        text-transform: uppercase;
    }
    :global(.dt-badge.ok) {
        color: var(--color-success);
        background: color-mix(in srgb, var(--color-success) 14%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-success) 45%, var(--color-border));
    }
    :global(.dt-badge.warn) {
        color: var(--color-warning);
        background: color-mix(in srgb, var(--color-warning) 14%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-warning) 45%, var(--color-border));
    }
    :global(.dt-badge.fail) {
        color: var(--color-error);
        background: color-mix(in srgb, var(--color-error) 14%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-error) 45%, var(--color-border));
    }

    /* Banners */
    :global(.dt-note) {
        padding: 9px 12px;
        font-size: 11.5px;
        line-height: 1.5;
        color: var(--color-muted);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }
    :global(.dt-error) {
        padding: 9px 12px;
        font-size: 12.5px;
        color: var(--color-error);
        white-space: pre-wrap;
        border: 1px solid color-mix(in srgb, var(--color-error) 45%, var(--color-border));
        border-radius: var(--radius-control, 8px);
        background: color-mix(in srgb, var(--color-error) 9%, transparent);
    }
    :global(.dt-section-label) {
        font-size: 11px;
        font-weight: 600;
        letter-spacing: 0.04em;
        text-transform: uppercase;
        color: var(--color-muted);
    }
    :global(.dt-pill) {
        padding: 2px 8px;
        font-size: 10.5px;
        border-radius: 999px;
        color: var(--color-accent);
        background: var(--color-accent-soft, color-mix(in srgb, var(--color-accent) 12%, transparent));
        border: 1px solid color-mix(in srgb, var(--color-accent) 30%, var(--color-border));
    }

    /* Text status colors (reusable) */
    :global(.dt-text-success) { color: var(--color-success); }
    :global(.dt-text-warning) { color: var(--color-warning); }
    :global(.dt-text-error) { color: var(--color-error); }
    :global(.dt-text-muted) { color: var(--color-muted); }

    @media (max-width: 1023px) {
        .dt-rail {
            flex-direction: row;
            flex-wrap: wrap;
        }
        .dt-rail-group {
            flex: 1 1 auto;
        }
    }
</style>
