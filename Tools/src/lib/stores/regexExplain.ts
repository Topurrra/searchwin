/*
  regexExplain — PURE tokenizer that breaks an ECMAScript regex pattern into
  labeled, colour-codable segments for the Regex Tool's "annotated breakdown"
  panel. Each token carries the source text, a category (for colouring), and a
  plain-English label (the hover tooltip). No DOM / Svelte deps → unit-testable
  (see regexExplain.test.ts).

  This is a pragmatic tokenizer, not a full parser: it recognises the common
  constructs and degrades gracefully (anything unrecognised becomes a literal),
  which is exactly right for an at-a-glance explanation.
*/

export type RegexTokenType =
    | 'anchor'
    | 'class'
    | 'quantifier'
    | 'group'
    | 'alternation'
    | 'escape'
    | 'backref'
    | 'literal';

export interface RegexToken {
    text: string;
    type: RegexTokenType;
    label: string;
}

const CLASS_ESCAPES: Record<string, string> = {
    d: 'Any digit (0-9)',
    D: 'Any non-digit',
    w: 'Any word character (letter, digit, or underscore)',
    W: 'Any non-word character',
    s: 'Any whitespace character',
    S: 'Any non-whitespace character',
};
const ANCHOR_ESCAPES: Record<string, string> = {
    b: 'Word boundary',
    B: 'Non-word boundary',
};
const NAMED_ESCAPES: Record<string, string> = {
    n: 'Newline',
    t: 'Tab',
    r: 'Carriage return',
    f: 'Form feed',
    v: 'Vertical tab',
    0: 'Null character',
};

function cap(s: string): string {
    return s.charAt(0).toUpperCase() + s.slice(1);
}

/** Human-readable description of the inside of a [...] character class. */
function describeClassInner(inner: string): string {
    // Turn "a-z0-9_" into "a-z, 0-9, _" — light touch, good enough at a glance.
    const parts: string[] = [];
    let i = 0;
    while (i < inner.length) {
        if (inner[i] === '\\') {
            parts.push(inner.slice(i, i + 2));
            i += 2;
            continue;
        }
        if (inner[i + 1] === '-' && inner[i + 2] && inner[i + 2] !== ']') {
            parts.push(`${inner[i]}-${inner[i + 2]}`);
            i += 3;
            continue;
        }
        parts.push(inner[i]);
        i += 1;
    }
    return parts.join(', ');
}

function escapeToken(pattern: string, i: number): RegexToken {
    const n = pattern[i + 1] ?? '';
    if (n in CLASS_ESCAPES) return { text: `\\${n}`, type: 'class', label: CLASS_ESCAPES[n] };
    if (n in ANCHOR_ESCAPES) return { text: `\\${n}`, type: 'anchor', label: ANCHOR_ESCAPES[n] };
    if (n in NAMED_ESCAPES) return { text: `\\${n}`, type: 'escape', label: NAMED_ESCAPES[n] };
    return { text: `\\${n}`, type: 'escape', label: `Literal "${n}"` };
}

/** Break an ECMAScript pattern into annotated tokens. */
export function explainPattern(pattern: string): RegexToken[] {
    const out: RegexToken[] = [];
    let literal = '';
    const flush = () => {
        if (!literal) return;
        out.push({
            text: literal,
            type: 'literal',
            label: literal.length === 1 ? `Literal character "${literal}"` : `Literal text "${literal}"`,
        });
        literal = '';
    };

    let i = 0;
    while (i < pattern.length) {
        const c = pattern[i];
        const rest = pattern.slice(i);

        // ── anchors / any-char / alternation ──────────────────────────────
        if (c === '^') { flush(); out.push({ text: '^', type: 'anchor', label: 'Start of the string (or line, with the m flag)' }); i++; continue; }
        if (c === '$') { flush(); out.push({ text: '$', type: 'anchor', label: 'End of the string (or line, with the m flag)' }); i++; continue; }
        if (c === '.') { flush(); out.push({ text: '.', type: 'class', label: 'Any character except a line break (unless the s flag is set)' }); i++; continue; }
        if (c === '|') { flush(); out.push({ text: '|', type: 'alternation', label: 'OR - match the expression on either side' }); i++; continue; }

        // ── groups ────────────────────────────────────────────────────────
        if (c === '(') {
            flush();
            let m: RegExpExecArray | null;
            if ((m = /^\(\?<([A-Za-z_]\w*)>/.exec(rest))) { out.push({ text: m[0], type: 'group', label: `Start of named capturing group "${m[1]}"` }); i += m[0].length; continue; }
            if (rest.startsWith('(?:')) { out.push({ text: '(?:', type: 'group', label: 'Start of a non-capturing group' }); i += 3; continue; }
            if (rest.startsWith('(?<=')) { out.push({ text: '(?<=', type: 'group', label: 'Positive lookbehind - preceded by' }); i += 4; continue; }
            if (rest.startsWith('(?<!')) { out.push({ text: '(?<!', type: 'group', label: 'Negative lookbehind - not preceded by' }); i += 4; continue; }
            if (rest.startsWith('(?=')) { out.push({ text: '(?=', type: 'group', label: 'Positive lookahead - followed by' }); i += 3; continue; }
            if (rest.startsWith('(?!')) { out.push({ text: '(?!', type: 'group', label: 'Negative lookahead - not followed by' }); i += 3; continue; }
            if ((m = /^\(\?([imsuxg]+)\)/.exec(rest))) { out.push({ text: m[0], type: 'group', label: `Inline flags: ${m[1]}` }); i += m[0].length; continue; }
            out.push({ text: '(', type: 'group', label: 'Start of a capturing group' }); i++; continue;
        }
        if (c === ')') { flush(); out.push({ text: ')', type: 'group', label: 'End of group' }); i++; continue; }

        // ── character class [...] ─────────────────────────────────────────
        if (c === '[') {
            flush();
            let j = i + 1;
            const neg = pattern[j] === '^';
            if (neg) j++;
            if (pattern[j] === ']') j++; // a ] right after [ is a literal ]
            while (j < pattern.length && pattern[j] !== ']') {
                if (pattern[j] === '\\') j++;
                j++;
            }
            const end = Math.min(j, pattern.length - 1);
            const text = pattern.slice(i, end + 1);
            const inner = text.slice(neg ? 2 : 1, -1);
            out.push({
                text,
                type: 'class',
                label: `${neg ? 'Any character NOT in' : 'Any single character in'} the set: ${describeClassInner(inner)}`,
            });
            i = end + 1;
            continue;
        }

        // ── quantifiers ───────────────────────────────────────────────────
        if (c === '*' || c === '+' || c === '?') {
            flush();
            const lazy = pattern[i + 1] === '?';
            const text = lazy ? c + '?' : c;
            const base = c === '*' ? 'zero or more times' : c === '+' ? 'one or more times' : 'zero or one time (optional)';
            out.push({ text, type: 'quantifier', label: `${cap(base)}${lazy ? ', as few as possible (lazy)' : ' (greedy)'}` });
            i += text.length;
            continue;
        }
        if (c === '{') {
            const m = /^\{(\d+)(,(\d*))?\}(\??)/.exec(rest);
            if (m) {
                flush();
                const min = m[1];
                const hasComma = m[2] !== undefined;
                const max = m[3];
                let desc: string;
                if (!hasComma) desc = `exactly ${min} time${min === '1' ? '' : 's'}`;
                else if (!max) desc = `${min} or more times`;
                else desc = `between ${min} and ${max} times`;
                out.push({ text: m[0], type: 'quantifier', label: `Repeat ${desc}${m[4] === '?' ? ', lazy' : ''}` });
                i += m[0].length;
                continue;
            }
        }

        // ── escapes / backreferences ──────────────────────────────────────
        if (c === '\\') {
            flush();
            let m: RegExpExecArray | null;
            if ((m = /^\\u\{[0-9a-fA-F]+\}/.exec(rest)) || (m = /^\\u[0-9a-fA-F]{4}/.exec(rest))) {
                out.push({ text: m[0], type: 'escape', label: `Unicode code point ${m[0].slice(2)}` });
                i += m[0].length;
                continue;
            }
            if ((m = /^\\x[0-9a-fA-F]{2}/.exec(rest))) {
                out.push({ text: m[0], type: 'escape', label: `Hex character 0x${m[0].slice(2)}` });
                i += m[0].length;
                continue;
            }
            if ((m = /^\\k<([A-Za-z_]\w*)>/.exec(rest))) {
                out.push({ text: m[0], type: 'backref', label: `Backreference to named group "${m[1]}"` });
                i += m[0].length;
                continue;
            }
            if ((m = /^\\([1-9]\d*)/.exec(rest))) {
                out.push({ text: m[0], type: 'backref', label: `Backreference to group ${m[1]}` });
                i += m[0].length;
                continue;
            }
            out.push(escapeToken(pattern, i));
            i += 2;
            continue;
        }

        // ── literal run ───────────────────────────────────────────────────
        literal += c;
        i++;
    }
    flush();
    return out;
}
