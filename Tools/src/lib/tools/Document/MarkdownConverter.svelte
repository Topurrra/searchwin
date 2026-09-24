<script lang="ts">
    /*
      Markdown Converter — the Documents-pack home for everything Markdown.

      ABSORBED the devkit Markdown panel (2026-07-23). That panel did live
      preview + HTML→Markdown but could only ever put text on the clipboard;
      it lived under Developer Tools, where a writer looking to turn notes
      into a Word file would never find it. Everything it did is here,
      unchanged, plus real file export.

      WHY THE STYLES ARE LOCAL. The old panel styled itself with `dt-*`
      classes that DevToolkit.svelte declares as `:global(...)` — they only
      exist while DevToolkit is mounted. Moving the markup alone would have
      produced a completely unstyled tool, so the handful of rules it used
      are ported here as scoped `mc-*` classes. Do not reintroduce `dt-*`
      here; this tool must not depend on another tool being open.

      EXPORT BACKEND is reused, not rewritten: `notes_export_html` and
      `notes_export_styled_pdf` already turn Markdown into a styled file for
      Notes, and `notes_export_docx` (added with this tool) writes the .docx
      bytes the PDF path was already building and discarding. Plain text is
      the one conversion done here, since it is pure string work with no
      files to resolve.
    */
    import { marked } from 'marked';
    import { invoke } from '@tauri-apps/api/core';
    import { dirname } from '@tauri-apps/api/path';
    import { save, open } from '@tauri-apps/plugin-dialog';
    import { readTextFile } from '@tauri-apps/plugin-fs';
    import { Copy, FileDown, FolderOpen } from '@lucide/svelte';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import { Button, ToolPanel } from '$lib/ui';
    // marked output is injected via {@html} — route it through the note
    // sanitizer so authored/pasted Markdown containing raw HTML (<script>,
    // onerror=, javascript: URLs) can't execute in the WebView.
    import { sanitizeNoteHtml } from '$lib/notes/preview';
    // Input + view mode live in module stores so they survive the workspace
    // {#key} nav-unmount (the same reason the devkit panel used them).
    import {
        markdownTab,
        markdownInput,
        markdownHtmlInput,
        markdownPreviewMode,
        markdownSourceDir,
    } from '$lib/stores/devkitPanels';

    marked.setOptions({ gfm: true, breaks: true });

    // Raw marked output, BEFORE sanitization — used for "Copy HTML" and the
    // HTML source view (a copy/inspect surface, never injected into the DOM).
    let renderedHtml = $derived.by(() => {
        try {
            return marked.parse($markdownInput) as string;
        } catch {
            return '<p style="color:red">Parse error</p>';
        }
    });

    // The ONLY value approved for {@html}.
    let safeHtml = $derived(sanitizeNoteHtml(renderedHtml));

    let mdFromHtml = $derived.by(() => {
        if (!$markdownHtmlInput.trim()) return '';
        return htmlToMarkdown($markdownHtmlInput);
    });

    function htmlToMarkdown(html: string): string {
        return html
            .replace(/<h([1-6])[^>]*>(.*?)<\/h\1>/gi, (_, n, t) => '\n' + '#'.repeat(Number(n)) + ' ' + stripTags(t) + '\n')
            .replace(/<strong[^>]*>(.*?)<\/strong>/gi, (_, t) => `**${stripTags(t)}**`)
            .replace(/<b[^>]*>(.*?)<\/b>/gi, (_, t) => `**${stripTags(t)}**`)
            .replace(/<em[^>]*>(.*?)<\/em>/gi, (_, t) => `*${stripTags(t)}*`)
            .replace(/<i[^>]*>(.*?)<\/i>/gi, (_, t) => `*${stripTags(t)}*`)
            .replace(/<code[^>]*>(.*?)<\/code>/gi, (_, t) => `\`${t}\``)
            .replace(/<a[^>]*href="([^"]*)"[^>]*>(.*?)<\/a>/gi, (_, href, text) => `[${stripTags(text)}](${href})`)
            .replace(/<img[^>]*src="([^"]*)"[^>]*alt="([^"]*)"[^>]*>/gi, (_, src, alt) => `![${alt}](${src})`)
            .replace(/<blockquote[^>]*>(.*?)<\/blockquote>/gis, (_, t) => '\n> ' + stripTags(t).trim().replace(/\n/g, '\n> ') + '\n')
            .replace(/<ul[^>]*>(.*?)<\/ul>/gis, (_: string, t: string) => t.replace(/<li[^>]*>(.*?)<\/li>/gis, (__: string, li: string) => `- ${stripTags(li).trim()}\n`) + '\n')
            .replace(/<ol[^>]*>(.*?)<\/ol>/gis, (_: string, t: string) => { let n = 0; return t.replace(/<li[^>]*>(.*?)<\/li>/gis, (__: string, li: string) => `${++n}. ${stripTags(li).trim()}\n`) + '\n'; })
            .replace(/<p[^>]*>(.*?)<\/p>/gis, (_, t) => '\n' + stripTags(t).trim() + '\n')
            .replace(/<br\s*\/?>/gi, '\n')
            .replace(/<hr\s*\/?>/gi, '\n---\n')
            .replace(/<pre[^>]*><code[^>]*>([\s\S]*?)<\/code><\/pre>/gi, (_, t) => '\n```\n' + t + '\n```\n')
            .replace(/<[^>]+>/g, '')
            .replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&').replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&nbsp;/g, ' ')
            .replace(/\n{3,}/g, '\n\n')
            .trim();
    }

    function stripTags(s: string): string {
        return s.replace(/<[^>]+>/g, '');
    }

    /** Markdown → plain text. Done here rather than in Rust because it is pure
     *  string work with no images or files to resolve. Strips syntax while
     *  KEEPING the human-readable content (link text survives, the URL does not). */
    function markdownToPlainText(md: string): string {
        return md
            .replace(/^---\n[\s\S]*?\n---\n/, '')            // YAML frontmatter
            .replace(/```[\s\S]*?```/g, (block) => block.replace(/```[a-zA-Z]*\n?/g, ''))
            .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')        // images → alt text
            .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')         // links → label
            .replace(/^#{1,6}\s+/gm, '')                     // headings
            .replace(/^\s*>\s?/gm, '')                       // blockquotes
            .replace(/^\s*[-*+]\s+/gm, '• ')                 // bullets
            .replace(/\*\*([^*]+)\*\*/g, '$1')
            .replace(/\*([^*]+)\*/g, '$1')
            .replace(/`([^`]+)`/g, '$1')
            .replace(/^\s*[-*_]{3,}\s*$/gm, '')              // rules
            .replace(/\n{3,}/g, '\n\n')
            .trim();
    }

    // ─── Clipboard actions (carried over from the devkit panel) ──────────
    async function copyHtml() {
        await navigator.clipboard.writeText(renderedHtml);
        toast('Copied HTML', 'success');
    }
    async function copyMd() {
        await navigator.clipboard.writeText(mdFromHtml);
        toast('Copied Markdown', 'success');
    }
    async function copySource() {
        await navigator.clipboard.writeText($markdownInput);
        toast('Copied Markdown source', 'success');
    }

    // ─── File in ────────────────────────────────────────────────────────
    let docTitle = $state('');

    async function openMarkdownFile() {
        const picked = await open({
            title: 'Open a Markdown file',
            multiple: false,
            filters: [{ name: 'Markdown', extensions: ['md', 'markdown', 'mdown', 'txt'] }],
        });
        if (typeof picked !== 'string') return;
        try {
            const markdown = await readTextFile(picked);
            const sourceDir = await dirname(picked);
            $markdownInput = markdown;
            $markdownSourceDir = sourceDir;
            docTitle = (picked.split(/[\\/]/).pop() ?? '').replace(/\.[^.]+$/, '');
            $markdownTab = 'md-to-html';
            toast('Loaded Markdown file', 'success');
        } catch (error) {
            errorToast("Couldn't open that file", error, {
                hint: 'Make sure it is a text file you have permission to read.',
            });
        }
    }

    // ─── File out ───────────────────────────────────────────────────────
    type Target = 'html' | 'pdf' | 'docx' | 'txt';
    let exporting = $state<Target | null>(null);

    const TARGETS: { id: Target; label: string; ext: string; name: string }[] = [
        { id: 'html', label: 'HTML', ext: 'html', name: 'HTML page' },
        { id: 'pdf', label: 'PDF', ext: 'pdf', name: 'PDF document' },
        { id: 'docx', label: 'Word', ext: 'docx', name: 'Word document' },
        { id: 'txt', label: 'Text', ext: 'txt', name: 'Plain text' },
    ];

    async function exportAs(target: Target) {
        if (exporting) return;
        const source = $markdownInput.trim();
        if (!source) {
            toast('Nothing to convert — add some Markdown first', 'info');
            return;
        }
        const spec = TARGETS.find((t) => t.id === target)!;
        const path = await save({
            title: `Export as ${spec.name}`,
            defaultPath: `${docTitle || 'document'}.${spec.ext}`,
            filters: [{ name: spec.name, extensions: [spec.ext] }],
        });
        if (!path) return;

        exporting = target;
        try {
            if (target === 'txt') {
                // No backend round-trip: plain text needs no image resolution.
                const { writeTextFile } = await import('@tauri-apps/plugin-fs');
                await writeTextFile(path, markdownToPlainText($markdownInput));
            } else {
                // These three share one options shape; the Rust structs are
                // `#[serde(rename_all = "camelCase")]`, hence outputPath (NOT
                // output_path — a snake_case key silently fails to bind).
                const command =
                    target === 'html'
                        ? 'notes_export_html'
                        : target === 'pdf'
                          ? 'notes_export_styled_pdf'
                          : 'notes_export_docx';
                await invoke<{ outputPath: string }>(command, {
                    options: {
                        markdown: $markdownInput,
                        outputPath: path,
                        title: docTitle || null,
                        baseDir: $markdownSourceDir,
                    },
                });
            }
            toast(`Exported as ${spec.label}`, 'success');
        } catch (error) {
            errorToast(`Couldn't export as ${spec.label}`, error, {
                hint: 'Check that the destination folder is writable and the file is not open elsewhere.',
            });
        } finally {
            exporting = null;
        }
    }
</script>

<div class="mc-panel">
    <div class="mc-head">
        <div class="mc-head-text">
            <h2 class="mc-title">Markdown Converter</h2>
            <p class="mc-desc">
                Write or paste Markdown, preview it live, and export to HTML, PDF, Word, or plain
                text — or convert HTML back into clean Markdown. Everything runs on-device.
            </p>
        </div>

        <div class="mc-seg" role="group" aria-label="Conversion direction">
            <button
                type="button"
                class="mc-seg-btn"
                class:is-active={$markdownTab === 'md-to-html'}
                aria-pressed={$markdownTab === 'md-to-html'}
                onclick={() => ($markdownTab = 'md-to-html')}
            >
                Markdown → HTML
            </button>
            <button
                type="button"
                class="mc-seg-btn"
                class:is-active={$markdownTab === 'html-to-md'}
                aria-pressed={$markdownTab === 'html-to-md'}
                onclick={() => ($markdownTab = 'html-to-md')}
            >
                HTML → Markdown
            </button>
        </div>
    </div>

    <div class="mc-toolbar">
        {#if $markdownTab === 'md-to-html'}
            <div class="mc-seg" role="group" aria-label="Preview layout">
                {#each [['split', 'Split'], ['preview', 'Preview'], ['source', 'HTML source']] as [v, l]}
                    <button
                        type="button"
                        class="mc-seg-btn"
                        class:is-active={$markdownPreviewMode === v}
                        aria-pressed={$markdownPreviewMode === v}
                        onclick={() => ($markdownPreviewMode = v as any)}
                    >
                        {l}
                    </button>
                {/each}
            </div>
            <Button variant="ghost" onclick={openMarkdownFile}>
                <FolderOpen size={16} /> Open .md
            </Button>
        {/if}

        <div class="mc-toolbar-end">
            {#if $markdownTab === 'md-to-html'}
                <Button variant="ghost" onclick={copySource}>
                    <Copy size={16} /> MD source
                </Button>
                <Button variant="ghost" onclick={copyHtml}>
                    <Copy size={16} /> Copy HTML
                </Button>
            {:else}
                <Button variant="primary" onclick={copyMd} disabled={!mdFromHtml}>
                    <Copy size={16} /> Copy Markdown
                </Button>
            {/if}
        </div>
    </div>

    {#if $markdownTab === 'md-to-html'}
        <!-- Export row: the reason this tool left Developer Tools. -->
        <div class="mc-export">
            <span class="mc-section-label">Export as</span>
            {#each TARGETS as t}
                <Button
                    variant={t.id === 'pdf' ? 'primary' : 'secondary'}
                    onclick={() => exportAs(t.id)}
                    disabled={exporting !== null || !$markdownInput.trim()}
                >
                    <FileDown size={16} />
                    {exporting === t.id ? 'Exporting…' : t.label}
                </Button>
            {/each}
        </div>

        <div class={$markdownPreviewMode === 'split' ? 'mc-two' : 'mc-col'}>
            {#if $markdownPreviewMode === 'split' || $markdownPreviewMode === 'source'}
                {#if $markdownPreviewMode === 'source'}
                    <div class="mc-io">
                        <div class="mc-io-head"><span class="mc-section-label">HTML output</span></div>
                        <textarea class="mc-ta is-tall mc-mono" value={renderedHtml} readonly spellcheck="false"
                        ></textarea>
                    </div>
                {:else}
                    <div class="mc-io">
                        <div class="mc-io-head"><span class="mc-section-label">Markdown input</span></div>
                        <textarea class="mc-ta is-tall mc-mono" bind:value={$markdownInput} spellcheck="false"
                        ></textarea>
                    </div>
                {/if}
            {/if}

            {#if $markdownPreviewMode === 'split' || $markdownPreviewMode === 'preview'}
                <div class="mc-io">
                    <div class="mc-io-head"><span class="mc-section-label">Preview</span></div>
                    <ToolPanel padding="md" scroll tone="panel-2">
                        <div class="md-body">
                            {@html safeHtml}
                        </div>
                    </ToolPanel>
                </div>
            {/if}
        </div>
    {:else}
        <div class="mc-two">
            <div class="mc-io">
                <div class="mc-io-head"><span class="mc-section-label">HTML input</span></div>
                <textarea
                    class="mc-ta is-tall mc-mono"
                    bind:value={$markdownHtmlInput}
                    spellcheck="false"
                    placeholder="Paste HTML here..."
                ></textarea>
            </div>
            <div class="mc-io">
                <div class="mc-io-head"><span class="mc-section-label">Markdown output</span></div>
                <textarea class="mc-ta is-tall mc-mono" value={mdFromHtml} readonly spellcheck="false"></textarea>
            </div>
        </div>
        <div class="mc-note">
            HTML → Markdown is best-effort. Complex layouts, tables, and nested structures may not
            convert perfectly.
        </div>
    {/if}
</div>

<style>
    /* Ported from DevToolkit's `:global(.dt-*)` rules so this tool stands on
       its own — see the header note. Scoped, not global. */
    .mc-panel {
        display: flex;
        flex-direction: column;
        gap: 16px;
        min-width: 0;
    }
    .mc-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        flex-wrap: wrap;
        padding-bottom: 10px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .mc-head-text {
        min-width: 0;
    }
    .mc-title {
        margin: 0;
        font-size: 15px;
        font-weight: 600;
        color: var(--color-text);
        letter-spacing: -0.01em;
    }
    .mc-desc {
        margin: 2px 0 0;
        font-size: 12px;
        line-height: 1.45;
        color: var(--color-muted);
        max-width: 74ch;
    }
    .mc-toolbar {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
        padding-bottom: 10px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .mc-toolbar-end {
        margin-left: auto;
        display: inline-flex;
        align-items: center;
        gap: 8px;
    }
    .mc-export {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
        padding: 0 0 10px;
        background: transparent;
        border: none;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        border-radius: 0;
    }
    .mc-two {
        display: grid;
        grid-template-columns: 1fr;
        gap: 12px;
        align-items: start;
    }
    @media (min-width: 980px) {
        .mc-two {
            grid-template-columns: 1fr 1fr;
        }
    }
    .mc-col {
        display: flex;
        flex-direction: column;
        gap: 12px;
        min-width: 0;
    }
    .mc-io {
        display: flex;
        flex-direction: column;
        gap: 6px;
        min-width: 0;
    }
    .mc-io-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
    }
    .mc-mono {
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
    }
    .mc-ta {
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
    .mc-ta.is-tall {
        min-height: 56vh;
    }
    .mc-seg {
        display: inline-flex;
        flex-wrap: wrap;
        max-width: 100%;
        padding: 3px;
        gap: 3px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel);
    }
    .mc-seg-btn {
        position: relative;
        display: inline-flex;
        align-items: center;
        gap: 6px;
        white-space: nowrap;
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
    .mc-seg-btn:hover {
        color: var(--color-text);
    }
    .mc-seg-btn.is-active {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .mc-seg-btn.is-active::before {
        content: '';
        position: absolute;
        left: 5px;
        top: 7px;
        bottom: 7px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .mc-seg-btn:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: -2px;
    }
    .mc-note {
        padding: 8px 0 8px 12px;
        font-size: 11.5px;
        line-height: 1.5;
        color: var(--color-muted);
        background: transparent;
        border: none;
        border-left: 2px solid color-mix(in srgb, var(--color-text) 18%, transparent);
        border-radius: 0;
    }
    .mc-section-label {
        font-size: 11px;
        font-weight: 600;
        letter-spacing: 0.04em;
        text-transform: uppercase;
        color: var(--color-muted);
    }

    /* Rendered Markdown body: semantic tokens only, no hardcoded color. */
    .md-body :global(h1) { font-size: 1.5rem; font-weight: 700; margin: 1rem 0 0.5rem; color: var(--color-text); }
    .md-body :global(h2) { font-size: 1.25rem; font-weight: 600; margin: 0.875rem 0 0.4rem; color: var(--color-text); }
    .md-body :global(h3) { font-size: 1.1rem; font-weight: 600; margin: 0.75rem 0 0.35rem; color: var(--color-text); }
    .md-body :global(h4), .md-body :global(h5), .md-body :global(h6) { font-weight: 600; margin: 0.5rem 0 0.25rem; color: var(--color-text); }
    .md-body :global(p) { margin: 0.5rem 0; line-height: 1.6; color: var(--color-text); }
    .md-body :global(ul), .md-body :global(ol) { padding-left: 1.5rem; margin: 0.5rem 0; color: var(--color-text); }
    .md-body :global(li) { margin: 0.2rem 0; color: var(--color-text); }
    .md-body :global(code) { background: var(--color-panel-2); border: 1px solid var(--color-border); border-radius: 3px; padding: 0.1rem 0.3rem; font-size: 0.85em; color: var(--color-accent); }
    .md-body :global(pre) { background: var(--color-panel-2); border: 1px solid var(--color-border); border-radius: 6px; padding: 1rem; overflow-x: auto; margin: 0.75rem 0; }
    .md-body :global(pre code) { background: none; border: none; padding: 0; color: var(--color-text); }
    .md-body :global(blockquote) { border-left: 3px solid var(--color-accent); padding-left: 1rem; margin: 0.75rem 0; color: var(--color-muted); }
    .md-body :global(a) { color: var(--color-accent); text-decoration: underline; }
    .md-body :global(hr) { border: none; border-top: 1px solid var(--color-border); margin: 1rem 0; }
    .md-body :global(table) { border-collapse: collapse; width: 100%; margin: 0.75rem 0; }
    .md-body :global(th), .md-body :global(td) { border: 1px solid var(--color-border); padding: 0.4rem 0.75rem; text-align: left; color: var(--color-text); }
    .md-body :global(th) { background: var(--color-panel-2); font-weight: 600; }
    .md-body :global(img) { max-width: 100%; height: auto; border-radius: 6px; }
    .md-body :global(strong) { font-weight: 700; color: var(--color-text); }
    .md-body :global(em) { font-style: italic; color: var(--color-text); }
</style>
