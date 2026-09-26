<!--
  Quality Pass Wave 1 / DF-4 (2026-05-29): inline preview pane that
  drops under any DuplicateFinder row when the user clicks the eye
  icon (or expands a whole group).

  Renders the right view per file kind — images use the Tauri asset://
  protocol (zero IPC), text/code uses highlight.js for syntax colour,
  PDF/DOCX/XLSX/PPTX/ODT/RTF show the extracted body text, audio/video
  ride the HTML5 native players, and anything else gets a hex dump of
  the first 256 bytes plus the metadata block.

  Lazy by design — the component only calls preview_duplicate_file
  when it actually mounts, so collapsing/expanding hundreds of rows
  costs nothing on idle.
-->
<script lang="ts">
    import { onMount } from 'svelte';
    import { invoke, convertFileSrc } from '@tauri-apps/api/core';
    import hljs from 'highlight.js';
    import {
        AlertCircle,
        Copy,
        ExternalLink,
        FileSearch,
        FolderOpen,
        ImageIcon,
        Loader2,
        Music,
        Video as VideoIcon,
        FileText,
    } from '@lucide/svelte';
    import { toast } from '$lib/stores/toasts';

    type Meta = {
        path: string;
        fileName: string;
        extension: string;
        sizeBytes: number;
        modifiedMs: number | null;
    };

    type Payload =
        | { kind: 'image'; meta: Meta }
        | { kind: 'audioVideo'; meta: Meta; mediaKind: 'audio' | 'video' }
        | {
            kind: 'text';
            meta: Meta;
            content: string;
            languageHint: string;
            truncated: boolean;
        }
        | { kind: 'document'; meta: Meta; content: string; truncated: boolean }
        | { kind: 'binary'; meta: Meta; hexPreview: string }
        | { kind: 'missing'; path: string; reason: string };

    // Svelte 5 props.
    let { path }: { path: string } = $props();

    let payload = $state<Payload | null>(null);
    let loadError = $state<string | null>(null);
    let loading = $state(true);

    // Image-specific natural dimensions (set onload). Shown as a
    // badge in the metadata row — useful when comparing the same
    // photo at different resolutions.
    let imgNaturalW = $state<number | null>(null);
    let imgNaturalH = $state<number | null>(null);

    onMount(async () => {
        try {
            const result = await invoke<Payload>('preview_duplicate_file', { path });
            payload = result;
        } catch (e) {
            loadError = e instanceof Error ? e.message : String(e);
        } finally {
            loading = false;
        }
    });

    function formatBytes(bytes: number): string {
        if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
        const units = ['B', 'KB', 'MB', 'GB', 'TB'];
        let size = bytes;
        let i = 0;
        while (size >= 1024 && i < units.length - 1) {
            size /= 1024;
            i += 1;
        }
        return `${size.toFixed(size >= 10 || i === 0 ? 0 : 1)} ${units[i]}`;
    }

    function formatModified(ms: number | null): string {
        if (!ms) return '—';
        try {
            return new Date(ms).toLocaleString();
        } catch {
            return '—';
        }
    }

    /** Run hljs over the content with the backend's language hint as a
     *  strong signal, falling back to auto-detect. Returns escaped /
     *  highlighted HTML safe to drop into a <pre> via {@html}. */
    function highlightCode(content: string, hint: string): string {
        try {
            if (hint && hljs.getLanguage(hint)) {
                return hljs.highlight(content, {
                    language: hint,
                    ignoreIllegals: true,
                }).value;
            }
            const auto = hljs.highlightAuto(content);
            return auto.value ?? escapeHtml(content);
        } catch {
            return escapeHtml(content);
        }
    }

    function escapeHtml(s: string): string {
        return s
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;');
    }

    async function copyPath() {
        try {
            await navigator.clipboard.writeText(path);
            toast('Path copied', 'success');
        } catch {
            toast('Could not copy path', 'error');
        }
    }

    async function openFile() {
        try {
            await invoke('host:open.file', { path });
        } catch (e) {
            toast(e instanceof Error ? e.message : String(e), 'error');
        }
    }

    async function revealInFolder() {
        try {
            const { revealItemInDir } = await import('@tauri-apps/plugin-opener');
            await revealItemInDir(path);
        } catch (e) {
            toast(e instanceof Error ? e.message : String(e), 'error');
        }
    }

    /**
     * Count the number of lines in the extracted text — useful badge
     * for code / log previews.
     */
    function lineCount(s: string): number {
        if (!s) return 0;
        // Count newlines; `+1` because the last line typically has no
        // trailing newline.
        let n = 1;
        for (let i = 0; i < s.length; i++) if (s.charCodeAt(i) === 10) n++;
        return n;
    }
</script>

<div class="dup-preview-card">
    {#if loading}
        <div class="dup-preview-loading">
            <Loader2 class="h-4 w-4 animate-spin" />
            <span>Loading preview…</span>
        </div>
    {:else if loadError}
        <div class="dup-preview-error">
            <AlertCircle class="h-4 w-4" />
            <span>Could not load preview: {loadError}</span>
        </div>
    {:else if payload}
        <!-- Metadata + actions header is the same shape for every
             kind — gives the user consistent muscle memory across the
             types. -->
        <div class="dup-preview-meta-row">
            <div class="dup-preview-meta-left">
                {#if payload.kind === 'image'}
                    <ImageIcon class="h-4 w-4 text-accent shrink-0" />
                {:else if payload.kind === 'audioVideo' && payload.mediaKind === 'audio'}
                    <Music class="h-4 w-4 text-accent shrink-0" />
                {:else if payload.kind === 'audioVideo'}
                    <VideoIcon class="h-4 w-4 text-accent shrink-0" />
                {:else if payload.kind === 'text' || payload.kind === 'document'}
                    <FileText class="h-4 w-4 text-accent shrink-0" />
                {:else}
                    <FileSearch class="h-4 w-4 text-accent shrink-0" />
                {/if}
                <div class="min-w-0">
                    <div class="text-xs font-medium truncate" title={payload.kind === 'missing' ? payload.path : payload.meta.fileName}>
                        {payload.kind === 'missing' ? 'File unavailable' : payload.meta.fileName}
                    </div>
                    {#if payload.kind !== 'missing'}
                        <div class="text-[10px] text-muted flex gap-2 flex-wrap">
                            <span>{formatBytes(payload.meta.sizeBytes)}</span>
                            <span aria-hidden="true">·</span>
                            <span>{formatModified(payload.meta.modifiedMs)}</span>
                            {#if payload.kind === 'image' && imgNaturalW && imgNaturalH}
                                <span aria-hidden="true">·</span>
                                <span>{imgNaturalW} × {imgNaturalH}</span>
                            {/if}
                            {#if payload.kind === 'text'}
                                <span aria-hidden="true">·</span>
                                <span>{lineCount(payload.content).toLocaleString()} lines</span>
                                {#if payload.languageHint}
                                    <span aria-hidden="true">·</span>
                                    <span class="font-mono">{payload.languageHint}</span>
                                {/if}
                            {/if}
                            {#if payload.kind === 'document'}
                                <span aria-hidden="true">·</span>
                                <span>extracted text</span>
                            {/if}
                            {#if payload.kind === 'binary'}
                                <span aria-hidden="true">·</span>
                                <span>binary · hex of first 256 B</span>
                            {/if}
                        </div>
                    {:else}
                        <div class="text-[10px] text-muted truncate">{payload.reason}</div>
                    {/if}
                </div>
            </div>
            <div class="dup-preview-actions">
                <button type="button" class="dup-preview-iconbtn" onclick={openFile} title="Open file">
                    <ExternalLink class="h-3.5 w-3.5" />
                </button>
                <button type="button" class="dup-preview-iconbtn" onclick={revealInFolder} title="Reveal in folder">
                    <FolderOpen class="h-3.5 w-3.5" />
                </button>
                <button type="button" class="dup-preview-iconbtn" onclick={copyPath} title="Copy path">
                    <Copy class="h-3.5 w-3.5" />
                </button>
            </div>
        </div>

        <!-- Body — kind-specific. -->
        {#if payload.kind === 'image'}
            <div class="dup-preview-image-wrap">
                <img
                    src={convertFileSrc(payload.meta.path)}
                    alt={payload.meta.fileName}
                    loading="lazy"
                    onload={(e) => {
                        const img = e.currentTarget as HTMLImageElement;
                        imgNaturalW = img.naturalWidth;
                        imgNaturalH = img.naturalHeight;
                    }}
                />
            </div>
        {:else if payload.kind === 'audioVideo' && payload.mediaKind === 'audio'}
            <audio controls src={convertFileSrc(payload.meta.path)} class="w-full">
                <track kind="captions" />
                Your browser does not support audio playback.
            </audio>
        {:else if payload.kind === 'audioVideo' && payload.mediaKind === 'video'}
            <video controls src={convertFileSrc(payload.meta.path)} class="w-full max-h-72 rounded-md">
                <track kind="captions" />
                Your browser does not support video playback.
            </video>
        {:else if payload.kind === 'text'}
            <pre class="dup-preview-pre"><code class="hljs language-{payload.languageHint || 'plaintext'}">{@html highlightCode(payload.content, payload.languageHint)}</code></pre>
            {#if payload.truncated}
                <div class="dup-preview-truncated">
                    Showing the first 16 KB. Open the file to see the rest.
                </div>
            {/if}
        {:else if payload.kind === 'document'}
            <pre class="dup-preview-pre dup-preview-pre--plain">{payload.content || '(no extractable text)'}</pre>
            {#if payload.truncated}
                <div class="dup-preview-truncated">
                    Showing the first 16 KB of extracted text.
                </div>
            {/if}
        {:else if payload.kind === 'binary'}
            <pre class="dup-preview-pre dup-preview-pre--hex">{payload.hexPreview}</pre>
        {:else if payload.kind === 'missing'}
            <div class="dup-preview-missing">
                File no longer available at this path. It may have been moved or deleted.
            </div>
        {/if}
    {/if}
</div>

<style>
    .dup-preview-card {
        border-radius: var(--rounded-lg, 0.5rem);
        border: 1px solid var(--border, rgb(255 255 255 / 8%));
        background: var(--panel-2, rgb(255 255 255 / 3%));
        padding: 0.5rem 0.625rem;
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        margin-top: 0.25rem;
    }

    .dup-preview-loading,
    .dup-preview-error,
    .dup-preview-missing {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        font-size: 0.75rem;
        color: var(--muted, rgb(255 255 255 / 60%));
    }
    .dup-preview-error {
        color: var(--danger, #e06c75);
    }
    .dup-preview-missing {
        color: var(--muted, rgb(255 255 255 / 60%));
        font-style: italic;
    }

    .dup-preview-meta-row {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        justify-content: space-between;
        min-width: 0;
    }
    .dup-preview-meta-left {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        min-width: 0;
        flex: 1;
    }
    .dup-preview-actions {
        display: flex;
        align-items: center;
        gap: 0.25rem;
        flex-shrink: 0;
    }
    .dup-preview-iconbtn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        border-radius: var(--rounded, 0.375rem);
        border: 1px solid var(--border, rgb(255 255 255 / 8%));
        background: transparent;
        color: var(--muted, rgb(255 255 255 / 60%));
        cursor: pointer;
        transition: border-color 120ms ease, color 120ms ease, background 120ms ease;
    }
    .dup-preview-iconbtn:hover {
        border-color: var(--accent, #60a5fa);
        color: var(--fg, #fff);
        background: var(--accent-soft, rgb(96 165 250 / 12%));
    }

    .dup-preview-image-wrap {
        display: flex;
        align-items: center;
        justify-content: center;
        background: var(--panel, rgb(0 0 0 / 25%));
        border-radius: var(--rounded, 0.375rem);
        padding: 0.25rem;
        max-height: 18rem;
        overflow: hidden;
    }
    .dup-preview-image-wrap img {
        max-height: 17rem;
        max-width: 100%;
        object-fit: contain;
        border-radius: var(--rounded-sm, 0.25rem);
    }

    .dup-preview-pre {
        margin: 0;
        max-height: 18rem;
        overflow: auto;
        padding: 0.5rem 0.625rem;
        background: var(--panel, rgb(0 0 0 / 25%));
        border-radius: var(--rounded, 0.375rem);
        font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
        font-size: 11.5px;
        line-height: 1.5;
        white-space: pre;
    }
    .dup-preview-pre--plain {
        white-space: pre-wrap;
        word-break: break-word;
    }
    .dup-preview-pre--hex {
        white-space: pre;
        letter-spacing: 0.02em;
    }

    .dup-preview-truncated {
        font-size: 10px;
        color: var(--muted, rgb(255 255 255 / 60%));
        font-style: italic;
        padding-left: 0.25rem;
    }
</style>
