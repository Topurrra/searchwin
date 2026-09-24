/*
  calcEngine — PURE Soulver-class calculation engine for the Calculator.

  A multi-line "paper": each line is parsed + evaluated independently, with a
  result on every line, variables that carry across lines, inline units (with
  conversion), natural percentages, running totals, and the classic scientific
  functions. No DOM / Svelte deps → fully unit-testable (see calcEngine.test.ts).

  Values are one of: a plain number, a quantity (number + unit), or a percent.
*/
import { resolveUnit, sameDimension, convert, unitDisplay, dimensionOf, type ResolvedUnit } from './calcUnits';

export type Value =
    | { t: 'num'; n: number }
    | { t: 'qty'; n: number; unit: ResolvedUnit }
    | { t: 'pct'; n: number } // n = the percent figure (15 means 15%)
    | { t: 'date'; ms: number }; // local-midnight epoch ms

// Resolved once for date arithmetic (date ± time-quantity, date − date).
const SECOND_UNIT = resolveUnit('s')!;
const DAY_UNIT = resolveUnit('day')!;

/** Local midnight today, offset by whole days. */
function todayMs(offsetDays = 0): number {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    d.setDate(d.getDate() + offsetDays);
    return d.getTime();
}

export interface EvalOptions {
    /** Trig in degrees (true, matches the classic calculator) or radians. */
    degrees?: boolean;
}

export interface LineResult {
    raw: string;
    kind: 'value' | 'assignment' | 'empty' | 'comment' | 'error';
    display: string; // formatted result for the right gutter ('' when nothing to show)
    value?: Value;
    varName?: string;
    error?: string;
}

// ─── Tokenizer ───────────────────────────────────────────────────────────────
type Tok =
    | { t: 'num'; v: number }
    | { t: 'op'; v: string }
    | { t: 'pct' }
    | { t: 'lpar' }
    | { t: 'rpar' }
    | { t: 'comma' }
    | { t: 'date'; ms: number }
    | { t: 'word'; v: string };

const KEYWORDS = new Set(['of', 'off', 'in', 'to', 'as']);

function tokenize(input: string): Tok[] {
    const s = input.replace(/×/g, '*').replace(/÷/g, '/').replace(/−/g, '-').replace(/π/g, 'pi');
    const toks: Tok[] = [];
    let i = 0;
    while (i < s.length) {
        const c = s[i];
        if (/\s/.test(c)) { i++; continue; }
        // ISO date literal YYYY-MM-DD (contiguous; spaced digits stay arithmetic).
        const dm = /^\d{4}-\d{2}-\d{2}/.exec(s.slice(i));
        if (dm) {
            const [Y, M, D] = dm[0].split('-').map(Number);
            toks.push({ t: 'date', ms: new Date(Y, M - 1, D).getTime() });
            i += dm[0].length;
            continue;
        }
        if (/[\d.]/.test(c)) {
            const start = i;
            let dots = 0;
            while (i < s.length && /[\d.]/.test(s[i])) { if (s[i] === '.') dots++; i++; }
            if (dots > 1) throw new Error('Invalid number');
            if (i < s.length && /[eE]/.test(s[i]) && /[\d+-]/.test(s[i + 1] ?? '')) {
                i++; if (/[+-]/.test(s[i])) i++;
                while (i < s.length && /\d/.test(s[i])) i++;
            }
            const v = Number(s.slice(start, i));
            if (!Number.isFinite(v)) throw new Error('Invalid number');
            toks.push({ t: 'num', v });
            continue;
        }
        if (/[a-zA-Z_°]/.test(c)) {
            const start = i;
            while (i < s.length && /[a-zA-Z_°0-9]/.test(s[i])) i++;
            toks.push({ t: 'word', v: s.slice(start, i) });
            continue;
        }
        if ('+-*/^!'.includes(c)) { toks.push({ t: 'op', v: c }); i++; continue; }
        if (c === '%') { toks.push({ t: 'pct' }); i++; continue; }
        if (c === '(') { toks.push({ t: 'lpar' }); i++; continue; }
        if (c === ')') { toks.push({ t: 'rpar' }); i++; continue; }
        if (c === ',') { toks.push({ t: 'comma' }); i++; continue; }
        throw new Error(`Unexpected character: ${c}`);
    }
    return toks;
}

// ─── Value helpers ───────────────────────────────────────────────────────────
/** Collapse any value to a plain number (percent → fraction; qty → magnitude). */
function asNum(v: Value): number {
    if (v.t === 'pct') return v.n / 100;
    if (v.t === 'date') return v.ms;
    return v.n;
}

function factorial(n: number): number {
    if (!Number.isInteger(n) || n < 0) throw new Error('Factorial needs a non-negative integer');
    if (n > 170) throw new Error('Factorial too large');
    let out = 1;
    for (let k = 2; k <= n; k++) out *= k;
    return out;
}

// ─── Parser (Pratt) ──────────────────────────────────────────────────────────
class Parser {
    private pos = 0;
    constructor(private toks: Tok[], private scope: Map<string, Value>, private ctx: { degrees: boolean; prev: () => Value; sum: () => Value }) {}

    parse(): Value {
        if (this.toks.length === 0) throw new Error('Empty');
        const v = this.expr(0);
        if (!this.end()) throw new Error('Invalid expression');
        return v;
    }

    private expr(minBp: number): Value {
        let lhs = this.prefix();
        // postfix: unit application, %, !, conversion
        for (;;) {
            const t = this.peek();
            if (!t) break;
            // A real unit directly after a number → quantity. ('in' resolves to
            // inches here; the conversion sense of in/to/as only fires below when
            // the left side is ALREADY a quantity, so there's no conflict.)
            if (t.t === 'word' && lhs.t === 'num') {
                const u = resolveUnit(t.v);
                if (u) { this.next(); lhs = { t: 'qty', n: lhs.n, unit: u }; continue; }
            }
            if (t.t === 'pct') {
                if (20 < minBp) break;
                this.next();
                lhs = { t: 'pct', n: asNumOrThrow(lhs) };
                continue;
            }
            if (t.t === 'op' && t.v === '!') {
                if (20 < minBp) break;
                this.next();
                lhs = { t: 'num', n: factorial(asNum(lhs)) };
                continue;
            }
            // conversion: in / to / as <unit>
            if (t.t === 'word' && (t.v.toLowerCase() === 'in' || t.v.toLowerCase() === 'to' || t.v.toLowerCase() === 'as')) {
                if (5 < minBp) break;
                const save = this.pos;
                this.next();
                const ut = this.peek();
                if (ut && ut.t === 'word') {
                    const target = resolveUnit(ut.v);
                    if (target && lhs.t === 'qty') {
                        this.next();
                        if (!sameDimension(lhs.unit, target)) throw new Error('Incompatible units');
                        lhs = { t: 'qty', n: convert(lhs.n, lhs.unit, target), unit: target };
                        continue;
                    }
                }
                this.pos = save; // not a conversion — leave the keyword for nobody (will error)
                break;
            }
            // of / off (percentage)
            if (t.t === 'word' && (t.v.toLowerCase() === 'of' || t.v.toLowerCase() === 'off')) {
                if (8 < minBp) break;
                const kw = t.v.toLowerCase();
                this.next();
                const rhs = this.expr(9);
                lhs = applyPercentKeyword(kw, lhs, rhs);
                continue;
            }
            if (t.t !== 'op') break;
            const bp = infixBp(t.v);
            if (!bp || bp.left < minBp) break;
            this.next();
            const rhs = this.expr(bp.right);
            lhs = applyInfix(t.v, lhs, rhs);
        }
        return lhs;
    }

    private prefix(): Value {
        const t = this.next();
        if (!t) throw new Error('Unexpected end');
        if (t.t === 'num') return { t: 'num', n: t.v };
        if (t.t === 'date') return { t: 'date', ms: t.ms };
        if (t.t === 'op' && t.v === '+') return this.expr(40);
        if (t.t === 'op' && t.v === '-') return negate(this.expr(40));
        if (t.t === 'lpar') {
            const v = this.expr(0);
            this.expect('rpar');
            return v;
        }
        if (t.t === 'word') {
            const name = t.v.toLowerCase();
            if (name === 'pi') return { t: 'num', n: Math.PI };
            if (name === 'tau') return { t: 'num', n: Math.PI * 2 };
            if (name === 'e') return { t: 'num', n: Math.E };
            if (name === 'today') return { t: 'date', ms: todayMs(0) };
            if (name === 'tomorrow') return { t: 'date', ms: todayMs(1) };
            if (name === 'yesterday') return { t: 'date', ms: todayMs(-1) };
            if (name === 'prev' || name === 'ans') return this.ctx.prev();
            if (name === 'sum' || name === 'total') return this.ctx.sum();
            // function call?
            if (this.peek()?.t === 'lpar') {
                this.next();
                const args: number[] = [];
                if (this.peek()?.t !== 'rpar') {
                    for (;;) {
                        args.push(asNum(this.expr(0)));
                        if (this.peek()?.t === 'comma') { this.next(); continue; }
                        break;
                    }
                }
                this.expect('rpar');
                return { t: 'num', n: applyFunction(name, args, this.ctx.degrees) };
            }
            // variable?
            const v = this.scope.get(name);
            if (v) return v;
            throw new Error(`Unknown name: ${t.v}`);
        }
        throw new Error('Unexpected token');
    }

    private peek(): Tok | undefined { return this.toks[this.pos]; }
    private next(): Tok | undefined { return this.toks[this.pos++]; }
    private end(): boolean { return this.pos >= this.toks.length; }
    private expect(t: Tok['t']) {
        const tok = this.next();
        if (!tok || tok.t !== t) throw new Error(t === 'rpar' ? 'Expected )' : `Expected ${t}`);
    }
}

function asNumOrThrow(v: Value): number {
    if (v.t === 'qty') throw new Error('Percent of a unit is ambiguous');
    if (v.t === 'date') throw new Error('Percent of a date is undefined');
    return v.n;
}

function infixBp(op: string): { left: number; right: number } | null {
    switch (op) {
        case '+': case '-': return { left: 10, right: 11 };
        case '*': case '/': return { left: 20, right: 21 };
        case '^': return { left: 30, right: 30 };
        default: return null;
    }
}

function negate(v: Value): Value {
    if (v.t === 'date') return v; // negating a date is meaningless — leave it
    if (v.t === 'qty') return { t: 'qty', n: -v.n, unit: v.unit };
    if (v.t === 'pct') return { t: 'pct', n: -v.n };
    return { t: 'num', n: -v.n };
}

/** Add a time-span to a date. Whole days advance the CALENDAR (DST-proof, so
 *  `Jan 1 + 90 days` lands on the right day regardless of clock changes); any
 *  sub-day remainder is added as real milliseconds. */
function addSpanToDate(ms: number, q: Value & { t: 'qty' }, sign: 1 | -1): number {
    const totalSec = convert(q.n, q.unit, SECOND_UNIT) * sign;
    const wholeDays = Math.trunc(totalSec / 86_400);
    const remSec = totalSec - wholeDays * 86_400;
    const d = new Date(ms);
    if (wholeDays) d.setDate(d.getDate() + wholeDays);
    return d.getTime() + remSec * 1000;
}

function applyDateInfix(op: string, a: Value, b: Value): Value {
    if (op === '+' && a.t === 'date' && b.t === 'qty' && dimensionOf(b.unit) === 'time')
        return { t: 'date', ms: addSpanToDate(a.ms, b, 1) };
    if (op === '+' && b.t === 'date' && a.t === 'qty' && dimensionOf(a.unit) === 'time')
        return { t: 'date', ms: addSpanToDate(b.ms, a, 1) };
    if (op === '-' && a.t === 'date' && b.t === 'qty' && dimensionOf(b.unit) === 'time')
        return { t: 'date', ms: addSpanToDate(a.ms, b, -1) };
    if (op === '-' && a.t === 'date' && b.t === 'date')
        return { t: 'qty', n: Math.round((a.ms - b.ms) / 86_400_000), unit: DAY_UNIT };
    throw new Error('Unsupported date operation');
}

function applyInfix(op: string, a: Value, b: Value): Value {
    if (a.t === 'date' || b.t === 'date') return applyDateInfix(op, a, b);
    // percentage in additive context: a ± b%  →  a ± (a * b/100)
    if ((op === '+' || op === '-') && b.t === 'pct') {
        const delta = asNum(a) * (b.n / 100);
        const n = op === '+' ? asNum(a) + delta : asNum(a) - delta;
        return a.t === 'qty' ? { t: 'qty', n, unit: a.unit } : { t: 'num', n };
    }
    if (op === '^') return { t: 'num', n: Math.pow(asNum(a), asNum(b)) };

    // unit-aware add/subtract
    if (op === '+' || op === '-') {
        if (a.t === 'qty' || b.t === 'qty') {
            const unit = a.t === 'qty' ? a.unit : (b as { unit: ResolvedUnit }).unit;
            const an = a.t === 'qty' ? a.n : asNum(a);
            let bn: number;
            if (b.t === 'qty') {
                if (a.t === 'qty' && !sameDimension(a.unit, b.unit)) throw new Error('Incompatible units');
                bn = a.t === 'qty' ? convert(b.n, b.unit, a.unit) : b.n;
            } else bn = asNum(b);
            return { t: 'qty', n: op === '+' ? an + bn : an - bn, unit };
        }
        return { t: 'num', n: op === '+' ? asNum(a) + asNum(b) : asNum(a) - asNum(b) };
    }

    // multiply / divide
    if (op === '*') {
        if (a.t === 'qty' && b.t === 'qty') throw new Error('Cannot multiply two units');
        if (a.t === 'qty') return { t: 'qty', n: a.n * asNum(b), unit: a.unit };
        if (b.t === 'qty') return { t: 'qty', n: asNum(a) * b.n, unit: b.unit };
        return { t: 'num', n: asNum(a) * asNum(b) };
    }
    if (op === '/') {
        if (asNum(b) === 0 && b.t !== 'qty') throw new Error('Division by zero');
        if (a.t === 'qty' && b.t === 'qty') {
            if (!sameDimension(a.unit, b.unit)) throw new Error('Incompatible units');
            return { t: 'num', n: a.n / convert(b.n, b.unit, a.unit) };
        }
        if (a.t === 'qty') return { t: 'qty', n: a.n / asNum(b), unit: a.unit };
        return { t: 'num', n: asNum(a) / asNum(b) };
    }
    throw new Error(`Unknown operator: ${op}`);
}

function applyPercentKeyword(kw: string, a: Value, b: Value): Value {
    const frac = a.t === 'pct' ? a.n / 100 : asNum(a); // "20% of x" or "0.2 of x"
    if (kw === 'of') {
        if (b.t === 'qty') return { t: 'qty', n: b.n * frac, unit: b.unit };
        return { t: 'num', n: asNum(b) * frac };
    }
    // "off": subtract that fraction of b from b
    if (b.t === 'qty') return { t: 'qty', n: b.n * (1 - frac), unit: b.unit };
    return { t: 'num', n: asNum(b) * (1 - frac) };
}

const DEG = new Set(['sin', 'cos', 'tan']);
function applyFunction(name: string, args: number[], degrees: boolean): number {
    const one = () => { if (args.length !== 1) throw new Error(`${name} expects 1 argument`); return args[0]; };
    const two = (): [number, number] => { if (args.length !== 2) throw new Error(`${name} expects 2 arguments`); return [args[0], args[1]]; };
    const toRad = (v: number) => (degrees ? (v * Math.PI) / 180 : v);
    const fromRad = (v: number) => (degrees ? (v * 180) / Math.PI : v);
    switch (name) {
        case 'sin': case 'cos': case 'tan': return Math[name](toRad(one()));
        case 'asin': return fromRad(Math.asin(one()));
        case 'acos': return fromRad(Math.acos(one()));
        case 'atan': return fromRad(Math.atan(one()));
        case 'sqrt': return Math.sqrt(one());
        case 'cbrt': return Math.cbrt(one());
        case 'ln': return Math.log(one());
        case 'log': return Math.log10(one());
        case 'exp': return Math.exp(one());
        case 'abs': return Math.abs(one());
        case 'floor': return Math.floor(one());
        case 'ceil': return Math.ceil(one());
        case 'round': return Math.round(one());
        case 'pow': { const [x, y] = two(); return Math.pow(x, y); }
        case 'root': { const [x, y] = two(); if (y === 0) throw new Error('Zero root'); return Math.pow(x, 1 / y); }
        case 'min': return Math.min(...args);
        case 'max': return Math.max(...args);
        default: throw new Error(`Unknown function: ${name}`);
    }
}

// ─── Formatting ──────────────────────────────────────────────────────────────
export function formatNumber(value: number): string {
    if (Object.is(value, -0)) return '0';
    if (!Number.isFinite(value)) throw new Error('Result is not finite');
    const abs = Math.abs(value);
    if ((abs !== 0 && abs < 1e-9) || abs >= 1e12) {
        return value.toExponential(6).replace(/\.?0+e/, 'e');
    }
    return new Intl.NumberFormat('en-US', { maximumFractionDigits: 10 }).format(Number(value.toPrecision(12)));
}

export function formatValue(v: Value): string {
    if (v.t === 'pct') return `${formatNumber(v.n)}%`;
    if (v.t === 'qty') {
        // Spelled-out time units read better pluralized ("59 days", not "59 day").
        let label = unitDisplay(v.unit);
        if ((label === 'day' || label === 'week') && Math.abs(v.n) !== 1) label += 's';
        return `${formatNumber(v.n)} ${label}`;
    }
    if (v.t === 'date') {
        const d = new Date(v.ms);
        const wd = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'][d.getDay()];
        const mm = String(d.getMonth() + 1).padStart(2, '0');
        const dd = String(d.getDate()).padStart(2, '0');
        return `${d.getFullYear()}-${mm}-${dd} (${wd})`;
    }
    return formatNumber(v.n);
}

// ─── Document evaluation ─────────────────────────────────────────────────────
const ASSIGN_RE = /^\s*([a-zA-Z_][a-zA-Z_0-9]*)\s*=\s*(.+)$/;

/** Evaluate a whole multi-line document. Variables carry forward; `prev`/`sum`
 *  reference earlier lines. Each line gets its own result row. */
export function evaluateDocument(text: string, opts: EvalOptions = {}): LineResult[] {
    const degrees = opts.degrees ?? true;
    const scope = new Map<string, Value>();
    const lineValues: (Value | null)[] = []; // numeric history for prev/sum
    const out: LineResult[] = [];

    for (const raw of text.split('\n')) {
        const trimmed = raw.trim();
        if (!trimmed) { out.push({ raw, kind: 'empty', display: '' }); lineValues.push(null); continue; }
        if (trimmed.startsWith('#') || trimmed.startsWith('//')) {
            out.push({ raw, kind: 'comment', display: '' });
            lineValues.push(null);
            continue;
        }

        const prevFn = (): Value => {
            for (let k = lineValues.length - 1; k >= 0; k--) if (lineValues[k]) return lineValues[k] as Value;
            return { t: 'num', n: 0 };
        };
        const sumFn = (): Value => {
            let acc = 0;
            for (const v of lineValues) if (v && v.t !== 'date') acc += asNum(v);
            return { t: 'num', n: acc };
        };

        const assign = ASSIGN_RE.exec(trimmed);
        const exprText = assign ? assign[2] : trimmed;

        try {
            const toks = tokenize(exprText);
            const value = new Parser(toks, scope, { degrees, prev: prevFn, sum: sumFn }).parse();
            const display = formatValue(value);
            if (assign) {
                scope.set(assign[1].toLowerCase(), value);
                out.push({ raw, kind: 'assignment', display, value, varName: assign[1] });
            } else {
                out.push({ raw, kind: 'value', display, value });
            }
            lineValues.push(value);
        } catch (e) {
            out.push({ raw, kind: 'error', display: '', error: e instanceof Error ? e.message : String(e) });
            lineValues.push(null);
        }
    }
    return out;
}

/** Single-expression convenience (e.g. the palette quick-calc). */
export function evaluateExpression(expr: string, opts: EvalOptions = {}): string {
    const res = evaluateDocument(expr, opts);
    const last = res[res.length - 1];
    if (!last || last.kind === 'error') throw new Error(last?.error ?? 'Error');
    return last.display;
}
