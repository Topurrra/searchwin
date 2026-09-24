/*
  notes/preview.ts — parse + render `.ki` notes for read-only preview surfaces
  (command palette, future quick-look, etc.).

  Why this module exists
  ──────────────────────
  Notes live as YAML-frontmatter + Markdown body in `.ki` files. The command
  palette's file reader used to dump the entire raw text into a `<pre>`, which
  meant users saw the frontmatter `--- ... ---` block before the actual content
  and the Markdown stayed unrendered ("# Title" instead of a real heading).

  Three responsibilities, kept separate:
    1. `parseNoteMarkdown(raw, filename?)` — pure parse. Mirrors the lenient
       reader in `stores/notes.ts` (which writes the canonical format), but
       returns a richer shape suited to a PREVIEW surface (creation/update
       timestamps as nullable so a `created: 0` doesn't render as 1970) and
       is exhaustively defensive — a malformed file never throws past this
       function.
    2. `renderNoteHtml(markdown, notePath?)` — Markdown → safe HTML. Uses
       `marked` (an existing dep) for parsing, `highlight.js` for code-block
       syntax highlighting (Wave K), then walks the result with `DOMParser`
       and drops anything dangerous: <script>, event-handler attrs,
       javascript: URLs, data: URLs, inline `style`. `<img src>` pointing
       at local files (`C:\…`, `file:///…`) is rewritten via Tauri's
       `convertFileSrc` so the WebView can load it. Returns a string ready
       for `{@html}`.
    3. `sanitizeNoteHtml(html, notePath?)` — the standalone sanitizer used
       by step 2. Exposed so future surfaces can reuse it without re-running
       `marked` (e.g. pasted HTML, an export preview).

  Safety contract
  ───────────────
  `renderNoteHtml`'s output is the ONLY thing approved for `{@html}` injection
  from note content. The sanitizer is tag-allowlisted and attribute-allowlisted
  — every tag must be in `ALLOWED_TAGS`, every attribute in `ALLOWED_ATTRS` for
  its tag. Anything outside is dropped. URLs in `href`/`src` are filtered to
  http/https/mailto/relative + Tauri-converted local file paths; data: +
  javascript: + vbscript: are rejected. Inline styles are dropped wholesale.

  Why we don't add DOMPurify: the markdown source is constrained (only `marked`
  + `hljs` output), `marked` itself escapes unsafe HTML by default, and our
  walk is a defense-in-depth layer that handles the user-authored-HTML case
  and stays < 200 lines — much smaller than pulling in a 40 KB sanitizer for
  one read-only surface.
*/
import { convertFileSrc } from '@tauri-apps/api/core';
import hljs from 'highlight.js';
import { marked } from 'marked';

// Marked configuration matches the editor (Notes.svelte / MarkdownPreview)
// so what the user sees IN the editor matches what they see in the preview.
// We also install a custom code renderer that runs `hljs` over the body —
// `marked` emits `<pre><code class="language-X">…</code></pre>` and the
// renderer rewrites the inner HTML to the token-span markup hljs produces.
// `tokens` is the parsed-block API; using it (vs the legacy string-renderer)
// keeps the surrounding `<pre><code>` wrapper marked emits by default so the
// CSS pipeline stays untouched.
marked.setOptions({ gfm: true, breaks: true });
marked.use({
    renderer: {
        // Marked v18 calls this with a token object: `{ type, raw, lang,
        // codeBlockStyle, text }`. We highlight the `text` payload and
        // wrap it in the canonical `<pre><code>` shape.
        code({ text, lang }) {
            const trimmedLang = (lang ?? '').trim().toLowerCase();
            let highlighted: string;
            let resolvedLang = trimmedLang;
            try {
                if (trimmedLang && hljs.getLanguage(trimmedLang)) {
                    highlighted = hljs.highlight(text, {
                        language: trimmedLang,
                        ignoreIllegals: true,
                    }).value;
                } else {
                    // No language hint (or unknown) — let hljs auto-detect.
                    // Auto-detection costs CPU but our previews are < 64 KB,
                    // so it's bounded.
                    const auto = hljs.highlightAuto(text);
                    highlighted = auto.value;
                    resolvedLang = auto.language ?? '';
                }
            } catch {
                // hljs failed (very rare) — fall back to escaped text.
                highlighted = escapeHtml(text);
            }
            const langClass = resolvedLang ? ` language-${escapeHtml(resolvedLang)}` : '';
            return `<pre><code class="hljs${langClass}">${highlighted}</code></pre>\n`;
        },
    },
});

export interface NotePreview {
    /** Title resolved via the fallback chain (frontmatter → first heading →
     *  filename → "Untitled note"). Never empty. */
    title: string;
    /** Body Markdown with the leading frontmatter block (if any) stripped.
     *  Pass to `renderNoteHtml` to get safe HTML, or render as-is in a
     *  code-block diagnostic surface. */
    bodyMarkdown: string;
    /** Creation timestamp in milliseconds. `null` when missing, malformed,
     *  or literal 0 — UIs should treat null as "unknown" rather than
     *  rendering "1970". */
    createdAt: number | null;
    /** Update timestamp in milliseconds. Same null semantics as `createdAt`. */
    updatedAt: number | null;
    /** Tags from the frontmatter `tags: [...]` array. Empty when absent. */
    tags: string[];
    /** True when the frontmatter `pinned: true`. */
    pinned: boolean;
    /** True when a `--- ... ---` block was found AND consumed at least one
     *  recognized key. False for notes without frontmatter, files where the
     *  block was malformed, or any parse error. UIs may use this to decide
     *  whether to show the metadata header row at all. */
    frontmatterValid: boolean;
}

/** Parse a `.ki` file string. Lenient — invalid frontmatter falls back to
 *  treating the whole text as body. Never throws past this function. */
export function parseNoteMarkdown(raw: string, filename?: string): NotePreview {
    const fallbackTitle = filenameToTitle(filename);
    const result: NotePreview = {
        title: '',
        bodyMarkdown: raw ?? '',
        createdAt: null,
        updatedAt: null,
        tags: [],
        pinned: false,
        frontmatterValid: false,
    };

    try {
        const text = (raw ?? '').replace(/\r\n/g, '\n');
        let body = text;
        let recognizedKeys = 0;

        if (text.startsWith('---\n')) {
            const end = text.indexOf('\n---', 4);
            if (end !== -1) {
                const yaml = text.slice(4, end);
                for (const line of yaml.split('\n')) {
                    const idx = line.indexOf(':');
                    if (idx === -1) continue;
                    const key = line.slice(0, idx).trim();
                    const val = line.slice(idx + 1).trim();
                    if (key === 'title') {
                        const t = yamlUnquote(val);
                        if (t) {
                            result.title = t;
                            recognizedKeys++;
                        }
                    } else if (key === 'created') {
                        const n = parseTimestamp(val);
                        // `created: 0` is the editor's pre-save sentinel — we
                        // explicitly map it to null so the UI can show "—"
                        // instead of "Jan 1 1970, 12:00 AM".
                        result.createdAt = Number.isFinite(n) && n > 0 ? n : null;
                        recognizedKeys++;
                    } else if (key === 'updated') {
                        const n = parseTimestamp(val);
                        result.updatedAt = Number.isFinite(n) && n > 0 ? n : null;
                        recognizedKeys++;
                    } else if (key === 'pinned') {
                        result.pinned = val === 'true';
                        recognizedKeys++;
                    } else if (key === 'tags') {
                        const inner = val.replace(/^\[/, '').replace(/\]$/, '');
                        result.tags = inner
                            .split(',')
                            .map((t) => yamlUnquote(t.trim()))
                            .filter(Boolean);
                        recognizedKeys++;
                    }
                }
                body = text.slice(end + 4).replace(/^\n+/, '');
                result.frontmatterValid = recognizedKeys > 0;
            }
        }

        result.bodyMarkdown = body;

        // Title fallback chain — frontmatter wins, then first H1/H2 in the
        // body, then filename, then "Untitled note". The chain prevents the
        // preview header from being empty for any well-formed OR malformed
        // input.
        if (!result.title) {
            const heading = body
                .split('\n')
                .find((l) => /^\s*#{1,6}\s+\S/.test(l));
            if (heading) {
                result.title = heading.replace(/^\s*#+\s+/, '').trim();
            }
        }
        if (!result.title) {
            result.title = fallbackTitle;
        }
    } catch {
        // Frontmatter parse blew up — keep raw as body, fall back on filename.
        result.bodyMarkdown = raw ?? '';
        result.frontmatterValid = false;
        result.title = fallbackTitle;
    }

    return result;
}

/** Render a Markdown body to safe HTML for `{@html}` injection. Returns
 *  empty string on parse failure (caller should render a plain-text
 *  fallback).
 *
 *  `notePath` (Wave K): absolute path to the note file on disk. Used to
 *  resolve relative `<img src>` paths (so a note pasted with
 *  `![](./photo.png)` shows the photo sitting next to it). Optional —
 *  without it, relative paths simply fail to load (no security risk;
 *  just a missing image). */
export function renderNoteHtml(bodyMarkdown: string, notePath?: string): string {
    if (!bodyMarkdown) return '';
    let rawHtml: string;
    try {
        // Marked returns a string when `async` is false (the default since
        // v7); the cast is for the TS overload, not a runtime promise.
        rawHtml = renderMarkdownWithNotesBlocks(bodyMarkdown);
    } catch {
        return '';
    }
    return sanitizeNoteHtml(rawHtml, notePath);
}

type PreviewCollapsible = { marker: string; title: string; body: string };

let previewCollapsibleId = 0;

/** Keep the preview readable without adding a second Markdown dependency. */
function renderMarkdownWithNotesBlocks(markdown: string): string {
    const { markdown: plainMarkdown, blocks } = extractCollapsibles(normalizeCalloutMarkers(markdown));
    let html = marked.parse(plainMarkdown) as string;

    for (const block of blocks) {
        const body = renderMarkdownWithNotesBlocks(block.body);
        const details = '<details><summary>' + escapeHtml(block.title) + '</summary><div>' + body + '</div></details>';
        html = html.replace(new RegExp('<p>' + block.marker + '</p>\\s*'), details);
    }
    return html;
}

function normalizeCalloutMarkers(markdown: string): string {
    let fence: string | null = null;
    return markdown.replace(/\r\n?/g, '\n').split('\n').map((line) => {
        const fenceToken = fenceAt(line);
        if (fenceToken) {
            if (!fence) fence = fenceToken;
            else if (closesFence(fence, fenceToken)) fence = null;
            return line;
        }
        if (fence) return line;
        return line.replace(/^\s*>\s*\[!(NOTE|TIP|WARNING|FILE)\]\s*/i, (_match, kind: string) => {
            const labels: Record<string, string> = {
                note: 'Note',
                tip: 'Tip',
                warning: 'Attention',
                file: 'Attachment',
            };
            const label = labels[kind.toLocaleLowerCase()] ?? 'Note';
            return '> **' + label + '** ';
        });
    }).join('\n');
}

function extractCollapsibles(markdown: string): { markdown: string; blocks: PreviewCollapsible[] } {
    const lines = markdown.split('\n');
    const result: string[] = [];
    const blocks: PreviewCollapsible[] = [];
    let fence: string | null = null;

    for (let index = 0; index < lines.length; index++) {
        const line = lines[index];
        const fenceToken = fenceAt(line);
        if (fenceToken) {
            if (!fence) fence = fenceToken;
            else if (closesFence(fence, fenceToken)) fence = null;
            result.push(line);
            continue;
        }
        if (fence) {
            result.push(line);
            continue;
        }

        const match = /^:::details(?:\s+(.+?))?\s*$/i.exec(line);
        if (!match) {
            result.push(line);
            continue;
        }

        const openingLine = line;
        const body: string[] = [];
        let nestedFence: string | null = null;
        let closed = false;
        while (++index < lines.length) {
            const current = lines[index];
            const nestedFenceToken = fenceAt(current);
            if (nestedFenceToken) {
                if (!nestedFence) nestedFence = nestedFenceToken;
                else if (closesFence(nestedFence, nestedFenceToken)) nestedFence = null;
                body.push(current);
                continue;
            }
            if (!nestedFence && current.trim() === ':::') {
                closed = true;
                break;
            }
            body.push(current);
        }
        if (!closed) {
            result.push(openingLine, ...body);
            break;
        }

        const marker = 'KILCOLLAPSIBLE' + (++previewCollapsibleId) + 'TOKEN';
        blocks.push({
            marker,
            title: match[1]?.trim().slice(0, 120) || 'Details',
            body: body.join('\n'),
        });
        result.push('', marker, '');
    }

    return { markdown: result.join('\n'), blocks };
}

function fenceAt(line: string): string | null {
    return new RegExp('^\\s*(' + String.fromCharCode(96) + '{3,}|~{3,})').exec(line)?.[1] ?? null;
}

function closesFence(opening: string, closing: string): boolean {
    return opening[0] === closing[0] && closing.length >= opening.length;
}

// ─── Sanitization ──────────────────────────────────────────────────────────

/** Tags allowed through the sanitizer. Anything else is unwrapped to its
 *  text content — the user sees the words, not the markup. */
const ALLOWED_TAGS = new Set([
    'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
    'p', 'blockquote', 'details', 'summary', 'hr', 'br',
    'ul', 'ol', 'li',
    'pre', 'code',
    'em', 'strong', 'b', 'i', 'del', 's', 'u',
    'a', 'img',
    'table', 'thead', 'tbody', 'tr', 'th', 'td',
    'mark', 'sup', 'sub',
    'div', 'span', // marked wraps some constructs; hljs token spans need `class`.
]);

/** Per-tag attribute allowlists. Everything else gets stripped — including
 *  every `on*` event handler, every `style`, every `id`, and every `data-*`. */
const ALLOWED_ATTRS: Record<string, ReadonlySet<string>> = {
    a: new Set(['href', 'title']),
    img: new Set(['src', 'alt', 'title']),
    code: new Set(['class']),
    pre: new Set(['class']),
    // Wave K (2026-05-28): hljs emits `<span class="hljs-keyword">` etc.
    // for syntax-highlighted tokens. Allowing `class` on span is purely
    // styling — `class` cannot execute code — and confined to whatever
    // hljs decides to output.
    span: new Set(['class']),
    th: new Set(['align']),
    td: new Set(['align']),
};

/** Explicit web/mail links plus relative URLs only. */
export function isSafeUrl(url: string): boolean {
    const trimmed = url.trim();
    if (!trimmed || trimmed.startsWith('//')) return false;
    if (/^(https?|mailto):/i.test(trimmed)) return true;
    return !/^[a-z][a-z0-9+.-]*:/i.test(trimmed);
}

/** Detect "this looks like a local file path I should convert to an
 *  asset URL" — both Windows absolute paths (`C:\…`, `D:/…`, `\\share\…`)
 *  and `file://…` URLs. Used only for `<img src>`; we don't auto-convert
 *  `<a href>` because clicking a link should still open the user's
 *  default app (handled by the link's target=_blank → external opener). */
function looksLikeLocalFilePath(url: string): boolean {
    const trimmed = url.trim();
    if (!trimmed) return false;
    if (/^file:\/\//i.test(trimmed)) return true;
    if (/^[a-z]:[\\/]/i.test(trimmed)) return true; // C:\... or C:/...
    if (trimmed.startsWith('\\\\')) return true; // UNC share \\server\…
    return false;
}

/** Resolve an `<img src>` to a Tauri asset URL when possible. Returns
 *  the rewritten URL, or the original when no conversion is needed
 *  (http/https/relative/etc.). Throws nothing — a malformed path just
 *  passes through unchanged.
 *
 *  `notePath` is the path to the note file on disk; used to resolve
 *  relative `<img src>` paths like `./photo.png` to an absolute path
 *  sitting next to the note. */
function resolveImageSrc(raw: string, notePath?: string): string {
    const trimmed = raw.trim();
    if (!trimmed) return raw;
    // Absolute local path / file: URL → convert to asset URL.
    if (looksLikeLocalFilePath(trimmed)) {
        try {
            // convertFileSrc accepts a raw filesystem path OR a file://
            // URL and returns an `asset:` URL the WebView can load.
            // Strip `file://` prefix because some Tauri versions expect
            // the bare path; the conversion handles both.
            const path = trimmed.replace(/^file:\/\/+/i, '');
            return convertFileSrc(path);
        } catch {
            return raw;
        }
    }
    // Relative path (no scheme, no leading slash) AND we know the note's
    // location → resolve against the note's directory.
    if (notePath && !/^[a-z][a-z0-9+.-]*:/i.test(trimmed) && !trimmed.startsWith('/')) {
        try {
            const noteDir = notePath.replace(/[\\/][^\\/]*$/, '');
            // Normalize relative prefix variants: `./foo`, `foo`, `\foo`.
            const rel = trimmed.replace(/^\.[\\/]+/, '').replace(/^[\\/]+/, '');
            // Use OS separator preference (Windows uses backslash).
            const sep = noteDir.includes('\\') ? '\\' : '/';
            return convertFileSrc(`${noteDir}${sep}${rel}`);
        } catch {
            return raw;
        }
    }
    return raw;
}

export function sanitizeNoteHtml(html: string, notePath?: string): string {
    if (!html || typeof document === 'undefined') return '';
    const doc = new DOMParser().parseFromString(html, 'text/html');
    walk(doc.body, doc, notePath);
    return doc.body.innerHTML;
}

function walk(node: Element, doc: Document, notePath?: string): void {
    // Iterate a snapshot — we mutate children mid-walk.
    for (const child of Array.from(node.children)) {
        const tag = child.tagName.toLowerCase();
        if (!ALLOWED_TAGS.has(tag)) {
            // Unwrap: replace the element with its text content. Keeps the
            // user's words visible without rendering the dangerous tag.
            const replacement = doc.createTextNode(child.textContent ?? '');
            child.replaceWith(replacement);
            continue;
        }
        const allowed = ALLOWED_ATTRS[tag] ?? null;
        for (const attr of Array.from(child.attributes)) {
            const name = attr.name.toLowerCase();
            // Belt + braces: drop every on* handler regardless of allowlist.
            if (name.startsWith('on')) {
                child.removeAttribute(attr.name);
                continue;
            }
            // Drop everything not in the per-tag allowlist (or if the tag
            // has no allowlist entry, drop ALL attrs).
            if (!allowed || !allowed.has(name)) {
                child.removeAttribute(attr.name);
                continue;
            }
            // URL-bearing attrs need an extra scheme check.
            if (name === 'href' && !isSafeUrl(attr.value)) {
                child.removeAttribute(attr.name);
                continue;
            }
            if (name === 'src') {
                // For `<img>`, rewrite local paths through convertFileSrc.
                // Other tags don't have a src attr in the allowlist, so
                // this branch is effectively `<img>`-only.
                if (looksLikeLocalFilePath(attr.value) || (notePath && !/^[a-z][a-z0-9+.-]*:/i.test(attr.value.trim()))) {
                    child.setAttribute('src', resolveImageSrc(attr.value, notePath));
                } else if (!isSafeUrl(attr.value)) {
                    child.removeAttribute(attr.name);
                }
            }
        }
        // External links: open in a new context and disown the opener so a
        // malicious target page can't navigate the palette window via
        // `window.opener`. Only applies to `<a href="...">` — Markdown
        // images stay unchanged.
        if (tag === 'a' && child.hasAttribute('href')) {
            child.setAttribute('target', '_blank');
            child.setAttribute('rel', 'noopener noreferrer');
        }
        walk(child, doc, notePath);
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

function yamlUnquote(s: string): string {
    const t = s.trim();
    if (
        t.length >= 2 &&
        ((t[0] === '"' && t.at(-1) === '"') || (t[0] === "'" && t.at(-1) === "'"))
    ) {
        return t.slice(1, -1).replace(/\\"/g, '"').replace(/\\\\/g, '\\');
    }
    return t;
}

function parseTimestamp(value: string): number {
    const raw = yamlUnquote(value);
    const numeric = Number(raw);
    if (Number.isFinite(numeric)) return numeric;
    const parsed = Date.parse(raw);
    return Number.isFinite(parsed) ? parsed : 0;
}

function filenameToTitle(filename: string | undefined): string {
    if (!filename) return 'Untitled note';
    const stripped = filename.replace(/\.ki$/i, '').trim();
    return stripped || 'Untitled note';
}

/** Minimal HTML escaper used by the hljs fallback path. Kept local —
 *  the rest of the module never touches user-supplied text without
 *  routing it through `marked` (which escapes by default) or the
 *  DOMParser-based sanitizer. */
function escapeHtml(s: string): string {
    return s
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');
}
