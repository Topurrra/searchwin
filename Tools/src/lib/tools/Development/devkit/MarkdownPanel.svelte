<script lang="ts">
    import { marked } from 'marked';
    import { Copy } from '@lucide/svelte';
    import { toast } from '$lib/stores/toasts';
    import { Button, ToolPanel } from '$lib/ui';
    // The marked output is injected via {@html} — route it through the existing
    // note sanitizer so pasted/authored Markdown containing raw HTML (e.g.
    // <script>, onerror=, javascript: URLs) can't execute in the WebView.
    import { sanitizeNoteHtml } from '$lib/notes/preview';
    // State lifted into a module store so input + view mode survive leaving and
    // returning to Developer Tools (panels remount fresh on navigation).
    import {
        markdownTab,
        markdownInput,
        markdownHtmlInput,
        markdownPreviewMode,
    } from '$lib/stores/devkitPanels';

    // Configure marked
    marked.setOptions({
        gfm: true,
        breaks: true,
    });

    // Raw marked output, BEFORE sanitization — used for "Copy HTML" / the HTML
    // source view (a copy/inspect surface, not injected into the DOM).
    let renderedHtml = $derived.by(() => {
        try {
            return marked.parse($markdownInput) as string;
        } catch {
            return '<p style="color:red">Parse error</p>';
        }
    });

    // Sanitized HTML — the ONLY value approved for {@html} injection. Strips
    // <script>, event-handler attrs, javascript:/data: URLs, inline styles.
    let safeHtml = $derived(sanitizeNoteHtml(renderedHtml));

    // Naive HTML to Markdown (best-effort)
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
</script>

<div class="dt-panel">
    <div class="dt-head">
        <div class="dt-head-text">
            <h2 class="dt-title">Markdown — preview &amp; convert HTML → Markdown</h2>
            <p class="dt-desc">
                Render Markdown to a live preview, or convert HTML into clean Markdown. Everything is processed on-device.
            </p>
        </div>

        <div class="dt-seg" role="group" aria-label="Conversion direction">
            <button
                type="button"
                class="dt-seg-btn"
                class:is-active={$markdownTab === 'md-to-html'}
                onclick={() => ($markdownTab = 'md-to-html')}
            >
                Markdown → HTML
            </button>
            <button
                type="button"
                class="dt-seg-btn"
                class:is-active={$markdownTab === 'html-to-md'}
                onclick={() => ($markdownTab = 'html-to-md')}
            >
                HTML → Markdown
            </button>
        </div>
    </div>

    <!-- Toolbar: view mode + actions -->
    <div class="dt-toolbar">
        {#if $markdownTab === 'md-to-html'}
            <div class="dt-seg" role="group" aria-label="Preview layout">
                {#each [['split', 'Split'], ['preview', 'Preview'], ['source', 'HTML source']] as [v, l]}
                    <button
                        type="button"
                        class="dt-seg-btn"
                        class:is-active={$markdownPreviewMode === v}
                        onclick={() => ($markdownPreviewMode = v as any)}
                    >
                        {l}
                    </button>
                {/each}
            </div>
        {/if}

        <div class="dt-toolbar-end">
            {#if $markdownTab === 'md-to-html'}
                <Button variant="ghost" onclick={copySource}>
                    <Copy size={16} /> MD source
                </Button>
                <Button variant="primary" onclick={copyHtml}>
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
        <div class={$markdownPreviewMode === 'split' ? 'dt-two' : 'dt-col'}>
            {#if $markdownPreviewMode === 'split' || $markdownPreviewMode === 'source'}
                {#if $markdownPreviewMode === 'source'}
                    <div class="dt-io">
                        <div class="dt-io-head"><span class="dt-section-label">HTML output</span></div>
                        <textarea
                            class="dt-ta is-tall dt-mono"
                            value={renderedHtml}
                            readonly
                            spellcheck="false"
                        ></textarea>
                    </div>
                {:else}
                    <div class="dt-io">
                        <div class="dt-io-head"><span class="dt-section-label">Markdown input</span></div>
                        <textarea
                            class="dt-ta is-tall dt-mono"
                            bind:value={$markdownInput}
                            spellcheck="false"
                        ></textarea>
                    </div>
                {/if}
            {/if}

            {#if $markdownPreviewMode === 'split' || $markdownPreviewMode === 'preview'}
                <div class="dt-io">
                    <div class="dt-io-head"><span class="dt-section-label">Preview</span></div>
                    <ToolPanel padding="md" scroll tone="panel-2">
                        <div class="md-body">
                            {@html safeHtml}
                        </div>
                    </ToolPanel>
                </div>
            {/if}
        </div>
    {:else}
        <!-- HTML → Markdown -->
        <div class="dt-two">
            <div class="dt-io">
                <div class="dt-io-head"><span class="dt-section-label">HTML input</span></div>
                <textarea
                    class="dt-ta is-tall dt-mono"
                    bind:value={$markdownHtmlInput}
                    spellcheck="false"
                    placeholder="Paste HTML here..."
                ></textarea>
            </div>
            <div class="dt-io">
                <div class="dt-io-head"><span class="dt-section-label">Markdown output</span></div>
                <textarea
                    class="dt-ta is-tall dt-mono"
                    value={mdFromHtml}
                    readonly
                    spellcheck="false"
                ></textarea>
            </div>
        </div>
        <div class="dt-note">
            HTML → Markdown is best-effort. Complex layouts, tables, and nested structures may not convert perfectly. For production use, consider Pandoc.
        </div>
    {/if}
</div>

<style>
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
