/*
  notesMarkdown — PURE Markdown helpers for the Notes pipeline.

  No DOM / Tauri / Svelte imports on purpose: these are string→string functions
  that are unit-tested directly (see notesMarkdown.test.ts). They cover:
    • escaped-HTML detection (corrupted notes)
    • outer ```markdown fence unwrapping (pasted AI/chat output)
    • "smart import" cleanup of Google-Docs / Word "export to Markdown" output
*/

/**
 * Is this body entity-encoded HTML markup (corrupted by an earlier paste bug) —
 * as opposed to Markdown that merely contains a stray `&lt;query&gt;` placeholder?
 * We require an escaped CLOSING tag (e.g. `&lt;/p&gt;`), the signature of real
 * markup that never appears in placeholder text.
 */
export function looksLikeEscapedHtml(s: string): boolean {
    return /&lt;\/[a-zA-Z][\w-]*&gt;/.test(s ?? '');
}

/**
 * If the WHOLE text is a single ```markdown / ```md fenced block (a common
 * artifact of pasting AI/chat output that the user wants rendered, not shown as
 * a literal code block), return its inner content; otherwise return as-is.
 * Deliberately narrow — a bare ``` fence or an inline fence is left untouched.
 */
export function stripOuterMarkdownFence(md: string): string {
    const t = (md ?? '').trim();
    const m = /^```(?:markdown|md)[ \t]*\r?\n([\s\S]*?)\r?\n```$/.exec(t);
    return m ? m[1] : md;
}

// ─── Smart import (Google-Docs / Word Markdown export cleanup) ───────────────

/** Split a Markdown table row "| a | b |" into trimmed cell strings. */
function splitTableRow(line: string): string[] {
    let s = line.trim();
    if (s.startsWith('|')) s = s.slice(1);
    if (s.endsWith('|')) s = s.slice(0, -1);
    return s.split('|').map((c) => c.trim());
}

/** A GFM separator row, e.g. `| --- | :--: |`. */
function isSeparatorRow(line: string): boolean {
    const cells = splitTableRow(line);
    return cells.length > 0 && cells.every((c) => /^:?-+:?$/.test(c.replace(/\s/g, '')));
}

/** Best-effort code language from the cell's content. */
function guessCodeLang(code: string): string {
    const head = code.trimStart();
    if (head.startsWith('{') || head.startsWith('[')) return 'json';
    if (/\b(CREATE|SELECT|INSERT|UPDATE|ALTER|DROP)\b/i.test(head.slice(0, 80))) return 'sql';
    return '';
}

/** Turn a faked-code table cell back into clean source text. */
function cleanCodeCell(cell: string): string {
    return cell
        .replace(/<br\s*\/?>/gi, '\n') // <br> → newline
        .replace(/&lt;/g, '<')
        .replace(/&gt;/g, '>')
        .replace(/&quot;/g, '"')
        .replace(/&#0?39;/g, "'")
        .replace(/&amp;/g, '&')
        .replace(/\\([\\`*_{}[\]()#+\-.!|>])/g, '$1'); // drop export's md-escaping
}

/** Transform one contiguous table block (array of `|`-prefixed lines). */
function transformTableBlock(block: string[]): string[] {
    const contentRows = block.filter((l) => !isSeparatorRow(l));
    const contentCells = contentRows.map(splitTableRow);
    const singleColumn = contentCells.length > 0 && contentCells.every((r) => r.length === 1);

    if (singleColumn) {
        const cellText = contentCells.map((r) => r[0]).join('\n');
        if (cellText.trim() === '') return []; // empty 1-col table → drop (export junk)
        if (/<br\s*\/?>/i.test(cellText)) {
            // A code block the exporter faked as a 1-cell table → real fence.
            const code = cleanCodeCell(cellText);
            const lang = guessCodeLang(code);
            return ['', '```' + lang, ...code.split('\n'), '```', ''];
        }
        return block; // legit single-column table — leave it
    }

    // Multi-column table: drop all-empty rows (export junk), keep the rest.
    const cleaned = block.filter((l) => {
        if (isSeparatorRow(l)) return true;
        return !splitTableRow(l).every((c) => c === '');
    });
    return cleaned.length >= 2 ? cleaned : block;
}

/**
 * "Smart import": clean Markdown produced by Google-Docs / Word "export to
 * Markdown" so it renders idiomatically in the editor:
 *   • single-column `<br>` tables (faked code blocks) → fenced code
 *   • empty tables / all-empty rows → removed
 * A no-op on clean Markdown, and idempotent.
 */
export function sanitizeImportedMarkdown(md: string): string {
    if (!md) return md;
    // Fast path: clean Markdown has no <br> and no rows of only-empty cells.
    if (!/<br/i.test(md) && !/^\s*\|(?:\s*\|)+\s*$/m.test(md)) return md;

    const lines = md.split('\n');
    const out: string[] = [];
    let i = 0;
    while (i < lines.length) {
        if (lines[i].trim().startsWith('|')) {
            const block: string[] = [];
            while (i < lines.length && lines[i].trim().startsWith('|')) {
                block.push(lines[i]);
                i++;
            }
            out.push(...transformTableBlock(block));
        } else {
            out.push(lines[i]);
            i++;
        }
    }
    // Collapse the extra blank lines our fence wrapping can introduce.
    return out.join('\n').replace(/\n{3,}/g, '\n\n');
}
