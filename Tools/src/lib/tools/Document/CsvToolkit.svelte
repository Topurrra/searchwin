<script lang="ts">
    /*
      CsvToolkit — one calm surface for every CSV / spreadsheet job.

      2026-06-05 elegance pass: the four CSV jobs (Merge, Clean, Split,
      → JSON) PLUS the former standalone "Excel ↔ CSV" converter are now
      folded into a single tool. The browser-style tab bar is gone; mode
      switching uses a left rail (the app's canonical selected-item
      pattern — panel-2 surface + accent pill strip + accent icon).

      Each mode is a body-only panel (no nested ToolPage). Panels are
      lazy-mounted on first visit and kept alive (hidden) so in-progress
      work — picked files, options, results — survives a mode switch.

      Shared widget styles live here as `:global(.csvk-*)` so all five
      panels compose from one source of truth instead of re-rolling CSS.
    */
    import { ArrowLeftRight, Combine, Sparkles, Scissors, FileJson, FileSpreadsheet } from '@lucide/svelte';
    import { ToolPage } from '$lib/ui';
    import ConvertPanel from './csvk/ConvertPanel.svelte';
    import MergePanel from './csvk/MergePanel.svelte';
    import CleanPanel from './csvk/CleanPanel.svelte';
    import SplitPanel from './csvk/SplitPanel.svelte';
    import JsonPanel from './csvk/JsonPanel.svelte';
    import { get } from 'svelte/store';
    import { csvToolkitMode, type CsvToolkitMode } from '$lib/stores/csvToolkit';

    type ModeId = CsvToolkitMode;

    const modes: { id: ModeId; label: string; sub: string; icon: typeof Combine }[] = [
        { id: 'convert', label: 'Convert', sub: 'Excel ⇄ CSV', icon: ArrowLeftRight },
        { id: 'merge', label: 'Merge', sub: 'Append many → one', icon: Combine },
        { id: 'clean', label: 'Clean', sub: 'Tidy messy exports', icon: Sparkles },
        { id: 'split', label: 'Split', sub: 'Chunk big files', icon: Scissors },
        { id: 'json', label: '→ JSON', sub: 'CSV to JSON / JSONL', icon: FileJson },
    ];

    const restoredMode = get(csvToolkitMode);
    let mode = $state<ModeId>(restoredMode);
    // Lazy-mount: a panel is created on first visit and then kept alive.
    let visited = $state<Record<ModeId, boolean>>({
        convert: restoredMode === 'convert',
        merge: restoredMode === 'merge',
        clean: restoredMode === 'clean',
        split: restoredMode === 'split',
        json: restoredMode === 'json',
    });

    function select(id: ModeId): void {
        csvToolkitMode.set(id);
        mode = id;
        if (!visited[id]) visited = { ...visited, [id]: true };
    }
</script>

<ToolPage
    icon={FileSpreadsheet}
    iconTint="#fb7185"
    title="CSV Toolkit"
    description="Every CSV and spreadsheet job in one place — convert to and from Excel, merge, clean, split, and export to JSON. All conversions run locally; nothing is uploaded."
    width="wide"
    fill={false}
>
    <div class="csvk-shell">
        <nav class="csvk-rail" aria-label="CSV Toolkit modes">
            {#each modes as m (m.id)}
                <button
                    type="button"
                    class="csvk-rail-btn"
                    class:is-active={mode === m.id}
                    aria-current={mode === m.id ? 'page' : undefined}
                    onclick={() => select(m.id)}
                >
                    <m.icon class="csvk-rail-ico" />
                    <span class="csvk-rail-text">
                        <span class="csvk-rail-label">{m.label}</span>
                        <span class="csvk-rail-sub">{m.sub}</span>
                    </span>
                </button>
            {/each}
        </nav>

        <main class="csvk-main">
            {#if visited.convert}
                <div class="csvk-pane" class:hidden={mode !== 'convert'}><ConvertPanel /></div>
            {/if}
            {#if visited.merge}
                <div class="csvk-pane" class:hidden={mode !== 'merge'}><MergePanel /></div>
            {/if}
            {#if visited.clean}
                <div class="csvk-pane" class:hidden={mode !== 'clean'}><CleanPanel /></div>
            {/if}
            {#if visited.split}
                <div class="csvk-pane" class:hidden={mode !== 'split'}><SplitPanel /></div>
            {/if}
            {#if visited.json}
                <div class="csvk-pane" class:hidden={mode !== 'json'}><JsonPanel /></div>
            {/if}
        </main>
    </div>
</ToolPage>

<style>
    /* ── Shell layout: left rail + main ── */
    .csvk-shell {
        display: grid;
        grid-template-columns: 1fr;
        gap: 16px;
        align-items: start;
    }
    @media (min-width: 1024px) {
        .csvk-shell {
            grid-template-columns: 208px minmax(0, 1fr);
        }
    }

    /* ── Mode rail (canonical selected pattern) ── */
    .csvk-rail {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 6px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
    }
    @media (max-width: 1023px) {
        .csvk-rail {
            flex-direction: row;
            flex-wrap: wrap;
        }
        .csvk-rail-btn {
            width: auto;
            flex: 1 1 auto;
        }
    }
    .csvk-rail-btn {
        position: relative;
        display: flex;
        align-items: center;
        gap: 10px;
        width: 100%;
        padding: 8px 10px 8px 14px;
        border: none;
        border-radius: var(--radius-control, 8px);
        background: transparent;
        color: var(--color-text-secondary);
        text-align: left;
        cursor: pointer;
        transition: background-color var(--dur-micro, 120ms) var(--ease-out, ease);
    }
    .csvk-rail-btn:hover {
        background: color-mix(in srgb, var(--color-text) 6%, transparent);
        color: var(--color-text);
    }
    .csvk-rail-btn:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: -2px;
    }
    .csvk-rail-btn.is-active {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .csvk-rail-btn.is-active::before {
        content: '';
        position: absolute;
        left: 4px;
        top: 8px;
        bottom: 8px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .csvk-rail-btn :global(.csvk-rail-ico) {
        flex: none;
        width: 17px;
        height: 17px;
        color: var(--color-muted);
    }
    .csvk-rail-btn.is-active :global(.csvk-rail-ico) {
        color: var(--color-accent);
    }
    .csvk-rail-text {
        display: flex;
        flex-direction: column;
        min-width: 0;
        line-height: 1.25;
    }
    .csvk-rail-label {
        font-size: 13px;
        font-weight: 500;
    }
    .csvk-rail-sub {
        font-size: 10.5px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .csvk-main {
        min-width: 0;
    }
    .csvk-pane {
        min-width: 0;
    }

    /* ════════════════════════════════════════════════════════════════
       Shared panel widgets — used by all five mode panels via :global.
       One source of truth for toolbars, fields, lists, tables, stats,
       progress, and result rows. Semantic tokens only.
       ════════════════════════════════════════════════════════════════ */

    /* Panel scaffold */
    :global(.csvk-panel) {
        display: flex;
        flex-direction: column;
        gap: 12px;
        min-width: 0;
    }
    :global(.csvk-head) {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 12px;
        flex-wrap: wrap;
    }
    :global(.csvk-head-text) {
        min-width: 0;
    }
    :global(.csvk-title) {
        margin: 0;
        font-size: 15px;
        font-weight: 600;
        color: var(--color-text);
        letter-spacing: -0.01em;
    }
    :global(.csvk-desc) {
        margin: 2px 0 0;
        font-size: 12px;
        line-height: 1.45;
        color: var(--color-muted);
        max-width: 70ch;
    }

    /* Toolbar (action row) */
    :global(.csvk-toolbar) {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
        padding-bottom: 10px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    :global(.csvk-toolbar-end) {
        margin-left: auto;
        display: inline-flex;
        align-items: center;
        gap: 8px;
    }

    /* Two-column body: main + options sidebar */
    :global(.csvk-grid) {
        display: grid;
        grid-template-columns: 1fr;
        gap: 12px;
        align-items: start;
    }
    @media (min-width: 1180px) {
        :global(.csvk-grid) {
            grid-template-columns: minmax(0, 1fr) 300px;
        }
    }
    :global(.csvk-col) {
        display: flex;
        flex-direction: column;
        gap: 12px;
        min-width: 0;
    }

    /* Fields */
    :global(.csvk-fields) {
        display: grid;
        gap: 10px;
    }
    :global(.csvk-fields.cols-2) {
        grid-template-columns: 1fr 1fr;
    }
    :global(.csvk-fields.cols-3) {
        grid-template-columns: 1fr 1fr 1fr;
    }
    :global(.csvk-field) {
        display: flex;
        flex-direction: column;
        gap: 5px;
        min-width: 0;
    }
    :global(.csvk-label) {
        font-size: 11.5px;
        color: var(--color-text-secondary);
    }
    :global(.csvk-input),
    :global(.csvk-select) {
        width: 100%;
        height: 32px;
        padding: 0 8px;
        font-size: 13px;
        color: var(--color-text);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }
    :global(.csvk-select) {
        cursor: pointer;
    }
    :global(.csvk-select option) {
        background: var(--color-panel);
        color: var(--color-text);
    }
    :global(.csvk-input:focus-visible),
    :global(.csvk-select:focus-visible) {
        outline: 2px solid var(--color-accent);
        outline-offset: 1px;
    }
    :global(.csvk-input:disabled),
    :global(.csvk-select:disabled) {
        opacity: 0.55;
    }
    :global(.csvk-checks) {
        display: flex;
        flex-direction: column;
        gap: 9px;
    }
    :global(.csvk-hint) {
        font-size: 11px;
        line-height: 1.5;
        color: var(--color-muted);
    }

    /* Segmented control (Convert direction) */
    :global(.csvk-seg) {
        display: inline-flex;
        padding: 3px;
        gap: 3px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel);
    }
    :global(.csvk-seg-btn) {
        position: relative;
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 5px 14px 5px 17px;
        border: none;
        border-radius: calc(var(--radius-control, 8px) - 2px);
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12.5px;
        font-weight: 500;
        cursor: pointer;
        transition: color var(--dur-micro, 120ms) var(--ease-out, ease);
    }
    :global(.csvk-seg-btn:hover) {
        color: var(--color-text);
    }
    :global(.csvk-seg-btn.is-active) {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    :global(.csvk-seg-btn.is-active)::before {
        content: '';
        position: absolute;
        left: 5px;
        top: 7px;
        bottom: 7px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    :global(.csvk-seg-btn:focus-visible) {
        outline: 2px solid var(--color-accent);
        outline-offset: -2px;
    }

    /* File / source lists */
    :global(.csvk-list) {
        display: flex;
        flex-direction: column;
        max-height: 18rem;
        overflow: auto;
        scrollbar-gutter: stable;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    :global(.csvk-row) {
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
    :global(.csvk-row:first-child) {
        border-top: none;
    }
    :global(button.csvk-row) {
        cursor: pointer;
    }
    :global(button.csvk-row:hover) {
        background: color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    /* Canonical selected pattern */
    :global(.csvk-row.is-selected) {
        background: var(--color-panel-2);
    }
    :global(.csvk-row.is-selected)::before {
        content: '';
        position: absolute;
        left: 2px;
        top: 7px;
        bottom: 7px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    :global(.csvk-row-main) {
        min-width: 0;
        flex: 1;
    }
    :global(.csvk-row-name) {
        display: block;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--color-text);
    }
    :global(.csvk-row.is-selected .csvk-row-name) {
        color: var(--color-text);
    }
    :global(.csvk-row-path) {
        display: block;
        font-size: 10.5px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        direction: rtl;
        text-align: left;
    }
    :global(.csvk-link-btn) {
        flex: none;
        padding: 2px 4px;
        font-size: 11.5px;
        color: var(--color-muted);
        background: transparent;
        border: none;
        cursor: pointer;
    }
    :global(.csvk-link-btn:hover:not(:disabled)) {
        color: var(--color-error);
    }
    :global(.csvk-link-btn:disabled) {
        opacity: 0.5;
        cursor: default;
    }

    /* Drop CTA (empty list area) */
    :global(.csvk-drop-cta) {
        width: 100%;
        padding: 22px;
        text-align: center;
        font-size: 13px;
        color: var(--color-muted);
        background: transparent;
        border: 1px dashed var(--color-border);
        border-radius: var(--radius-control, 8px);
        cursor: pointer;
    }
    :global(.csvk-drop-cta:hover) {
        color: var(--color-text);
        border-color: color-mix(in srgb, var(--color-accent) 60%, var(--color-border));
    }

    /* Stat cards */
    :global(.csvk-stats) {
        display: grid;
        gap: 8px;
        grid-template-columns: repeat(2, 1fr);
    }
    @media (min-width: 900px) {
        :global(.csvk-stats.cols-4) {
            grid-template-columns: repeat(4, 1fr);
        }
        :global(.csvk-stats.cols-3) {
            grid-template-columns: repeat(3, 1fr);
        }
    }
    :global(.csvk-stat) {
        padding: 9px 11px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    :global(.csvk-stat-label) {
        font-size: 10.5px;
        color: var(--color-muted);
    }
    :global(.csvk-stat-val) {
        margin-top: 2px;
        font-size: 17px;
        font-weight: 600;
        color: var(--color-text);
        font-variant-numeric: tabular-nums;
    }
    :global(.csvk-stat-val.is-accent) {
        color: var(--color-accent);
    }
    :global(.csvk-stat-val.is-success) {
        color: var(--color-success);
    }

    /* Progress */
    :global(.csvk-prog) {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    :global(.csvk-prog-head) {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        font-size: 12.5px;
    }
    :global(.csvk-prog-main) {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
        color: var(--color-text);
    }
    :global(.csvk-prog-item) {
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    :global(.csvk-prog-meta) {
        flex: none;
        font-size: 11px;
        color: var(--color-muted);
        font-variant-numeric: tabular-nums;
    }
    :global(.csvk-bar) {
        height: 6px;
        border-radius: 999px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        overflow: hidden;
    }
    :global(.csvk-bar-fill) {
        height: 100%;
        border-radius: 999px;
        background: var(--color-accent);
        transition: width 240ms var(--ease-out, ease);
    }
    :global(.csvk-bar-fill.is-indeterminate) {
        width: 33% !important;
        animation: csvk-slide 1.15s ease-in-out infinite;
    }
    @keyframes csvk-slide {
        0% { transform: translate3d(-120%, 0, 0); }
        50% { transform: translate3d(115%, 0, 0); }
        100% { transform: translate3d(260%, 0, 0); }
    }

    /* Preview table */
    :global(.csvk-table-wrap) {
        max-height: 56vh;
        overflow: auto;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }
    :global(.csvk-table) {
        width: 100%;
        border-collapse: collapse;
        font-size: 11.5px;
    }
    :global(.csvk-table td) {
        padding: 4px 8px;
        max-width: 180px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--color-muted);
        border-top: 1px solid color-mix(in srgb, var(--color-border) 55%, transparent);
        vertical-align: top;
    }
    :global(.csvk-table tr:first-child td) {
        border-top: none;
    }
    :global(.csvk-table td.is-header) {
        font-weight: 600;
        color: var(--color-text);
    }

    /* Result rows */
    :global(.csvk-results) {
        display: flex;
        flex-direction: column;
        gap: 6px;
        max-height: 22rem;
        overflow: auto;
        scrollbar-gutter: stable;
    }
    :global(.csvk-result) {
        padding: 9px 11px;
        font-size: 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    :global(.csvk-result.is-fail) {
        border-color: color-mix(in srgb, var(--color-error) 45%, var(--color-border));
        background: color-mix(in srgb, var(--color-error) 9%, transparent);
    }
    :global(.csvk-result-row) {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 10px;
    }
    :global(.csvk-result-name) {
        min-width: 0;
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--color-text);
    }
    :global(.csvk-result-detail) {
        margin-top: 3px;
        font-size: 10.5px;
        color: var(--color-muted);
    }
    :global(.csvk-result-path) {
        margin-top: 3px;
        font-size: 10.5px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        direction: rtl;
        text-align: left;
    }
    :global(.csvk-badge) {
        flex: none;
        padding: 1px 7px;
        border-radius: 6px;
        font-size: 9.5px;
        font-weight: 700;
        letter-spacing: 0.03em;
        text-transform: uppercase;
    }
    :global(.csvk-badge.ok) {
        color: var(--color-success);
        background: color-mix(in srgb, var(--color-success) 14%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-success) 45%, var(--color-border));
    }
    :global(.csvk-badge.fail) {
        color: var(--color-error);
        background: color-mix(in srgb, var(--color-error) 14%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-error) 45%, var(--color-border));
    }
    :global(.csvk-result-error) {
        margin-top: 3px;
        font-size: 11px;
        color: var(--color-error);
    }

    /* Inline error banner */
    :global(.csvk-error) {
        padding: 9px 12px;
        font-size: 12.5px;
        color: var(--color-error);
        white-space: pre-wrap;
        border: 1px solid color-mix(in srgb, var(--color-error) 45%, var(--color-border));
        border-radius: var(--radius-control, 8px);
        background: color-mix(in srgb, var(--color-error) 9%, transparent);
    }

    /* Section label */
    :global(.csvk-section-label) {
        font-size: 11px;
        font-weight: 600;
        letter-spacing: 0.04em;
        text-transform: uppercase;
        color: var(--color-muted);
    }
    :global(.csvk-pill) {
        padding: 2px 8px;
        font-size: 10.5px;
        border-radius: 999px;
        color: var(--color-accent);
        background: var(--color-accent-soft, color-mix(in srgb, var(--color-accent) 12%, transparent));
        border: 1px solid color-mix(in srgb, var(--color-accent) 30%, var(--color-border));
    }

    @media (prefers-reduced-motion: reduce) {
        :global(.csvk-bar-fill) {
            transition: none;
        }
        :global(.csvk-bar-fill.is-indeterminate) {
            animation: none;
            width: 100% !important;
            opacity: 0.5;
        }
    }
</style>
