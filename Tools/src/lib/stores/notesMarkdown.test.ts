import { describe, it, expect } from 'vitest';
import { looksLikeEscapedHtml, stripOuterMarkdownFence, sanitizeImportedMarkdown } from './notesMarkdown';

describe('looksLikeEscapedHtml', () => {
    it('flags entity-encoded markup with a closing tag', () => {
        expect(looksLikeEscapedHtml('&lt;p&gt;&lt;strong&gt;Hi&lt;/strong&gt;&lt;/p&gt;')).toBe(true);
    });
    it('ignores placeholder text like &lt;query&gt; (no closing tag)', () => {
        expect(looksLikeEscapedHtml('q=&lt;query&gt;&hl=&lt;lang&gt;')).toBe(false);
    });
    it('ignores clean markdown', () => {
        expect(looksLikeEscapedHtml('# Title\n\n**bold**')).toBe(false);
    });
});

describe('stripOuterMarkdownFence', () => {
    it('unwraps a whole-body ```markdown fence', () => {
        expect(stripOuterMarkdownFence('```markdown\n# Hi\n\ntext\n```')).toBe('# Hi\n\ntext');
    });
    it('unwraps a ```md fence', () => {
        expect(stripOuterMarkdownFence('```md\nhello\n```')).toBe('hello');
    });
    it('leaves a normal note untouched', () => {
        expect(stripOuterMarkdownFence('# Hi\n\n```js\ncode\n```')).toBe('# Hi\n\n```js\ncode\n```');
    });
});

describe('sanitizeImportedMarkdown', () => {
    it('is a no-op on plain markdown', () => {
        const md = '# Title\n\nSome **bold** text and a [link](https://x.com).';
        expect(sanitizeImportedMarkdown(md)).toBe(md);
    });

    it('leaves clean multi-column tables unchanged', () => {
        const t = '| A | B |\n| --- | --- |\n| 1 | 2 |';
        expect(sanitizeImportedMarkdown(t)).toBe(t);
    });

    it('converts a single-column <br> JSON table into a fenced code block', () => {
        const md = '| {<br>  "a": 1,<br>  "b": 2<br>} |\n| --- |';
        const out = sanitizeImportedMarkdown(md);
        expect(out).toContain('```json');
        expect(out).toContain('"a": 1,');
        expect(out).not.toMatch(/<br/i);
        expect(out).not.toContain('| ---');
    });

    it('detects SQL fences', () => {
        const md = '| CREATE TABLE x (<br>  id UUID PRIMARY KEY<br>); |\n| --- |';
        expect(sanitizeImportedMarkdown(md)).toContain('```sql');
    });

    it('drops export md-escaping and decodes entities inside code cells', () => {
        const md = '| themes TEXT\\[\\] NOT NULL,<br>  q=&lt;query&gt;&amp;hl=en,<br>  source\\_type TEXT |\n| --- |';
        const out = sanitizeImportedMarkdown(md);
        expect(out).toContain('TEXT[]');
        expect(out).toContain('source_type');
        expect(out).toContain('<query>');
        expect(out).toContain('&hl=en');
        expect(out).not.toContain('\\_');
    });

    it('drops an empty single-column table', () => {
        expect(sanitizeImportedMarkdown('|  |\n| --- |').trim()).toBe('');
    });

    it('drops all-empty rows from a multi-column table', () => {
        const t = '| A | B |\n| --- | --- |\n| 1 | 2 |\n|  |  |';
        const out = sanitizeImportedMarkdown(t);
        expect(out).toContain('| 1 | 2 |');
        expect(out).not.toMatch(/^\s*\|\s*\|\s*\|\s*$/m);
    });

    it('is idempotent', () => {
        const md = '| {<br>  "a": 1<br>} |\n| --- |';
        const once = sanitizeImportedMarkdown(md);
        expect(sanitizeImportedMarkdown(once)).toBe(once);
    });
});
