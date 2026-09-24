/**
 * Frontend "quick-action" evaluators for the command palette (/command).
 *
 * The palette runs these BEFORE invoking the Rust backend's
 * `evaluate_quick_query` command — frontend matches win because their
 * parsers are more specific and the output formatting is nicer.
 * Anything the frontend chain returns null for falls through to the
 * backend's generic calculator + unit-conversion logic.
 *
 * Architecture: each evaluator is a pure function that takes the raw
 * query string and returns either a `RenderableQuickAction` (a
 * tagged-union shape the palette's split-card UI knows how to render)
 * or `null` (no match — try the next evaluator). The orchestrator
 * `evaluateFrontendQuickAction` runs them in order and returns the
 * first match.
 *
 * Why frontend at all: zero IPC cost (~0.5 ms per backend call), and
 * features like color conversion, date math, percentage operations,
 * and bitwise ops are awkward to express in the Rust expression
 * evaluator. JS handles them naturally.
 *
 * Local-first: no network calls. Everything is computed from the
 * input string plus the current Date / Math globals.
 */

// ─── Public types ────────────────────────────────────────────────────

/** Action variants the /command palette knows how to render AND
 *  activate from a single Enter press. Frontend evaluators (in this
 *  module) only produce `calculator` and `unitConversion`. The
 *  backend's `evaluate_quick_query` can also produce `openUrl` and
 *  `webSearch` (bang shortcuts like `g foo`, `?term`, `gh repo`),
 *  which the palette renders as Globe-iconed rows. `systemCommand`
 *  is still excluded — it needs a confirm-dialog flow that doesn't
 *  fit a quick keyboard activation.
 *
 *  Adding new variants here means: bump the activation switch in
 *  `+page.svelte::activateQuickAction`, the row template branch, the
 *  selectables prefix check, and the action-panel `actionsForSelected`
 *  branch. The type system catches missing cases via exhaustiveness. */
export type RenderableQuickAction =
    | { type: 'calculator'; expression: string; result: string }
    | { type: 'unitConversion'; original: string; result: string }
    | { type: 'openUrl'; url: string; display: string }
    | { type: 'webSearch'; provider: string; url: string; query: string }
    | {
          type: 'systemCommand';
          id: string;
          name: string;
          description: string;
          requiresConfirmation: boolean;
      };

// ─── Orchestrator ────────────────────────────────────────────────────

/**
 * Try every frontend evaluator in priority order. First match wins.
 *
 * Order rationale:
 *   1. Most specific / unambiguous patterns first (percentage, base
 *      conversion, date math) so their templates don't get swallowed
 *      by the generic math evaluator.
 *   2. Color, time, bitwise — distinct prefixes / shapes.
 *   3. Utility helpers (stats, tip, random, string ops) — keyword-led.
 *   4. Advanced math last — it accepts the widest input shape
 *      (anything that looks like a math expression), so it would
 *      otherwise swallow earlier patterns.
 */
export function evaluateFrontendQuickAction(value: string): RenderableQuickAction | null {
    return (
        evalPercentage(value) ??
        evalBaseConversion(value) ??
        evalDateMath(value) ??
        evalNaturalDate(value) ??
        evalDayOfWeek(value) ??
        evalAge(value) ??
        evalWorkdays(value) ??
        evalTimeArithmetic(value) ??
        evalTimeRange(value) ??
        evalColorConversion(value) ??
        evalColorContrast(value) ??
        evalRandomColor(value) ??
        evalBitwise(value) ??
        evalStringOps(value) ??
        evalUuid(value) ??
        evalRandomRange(value) ??
        evalTipCalc(value) ??
        evalDiscount(value) ??
        evalQuickStats(value) ??
        evalAdvancedMath(value)
    );
}

// ─── Shared helpers ──────────────────────────────────────────────────

/** Trim trailing IEEE-754 zeros from a number's string representation
 *  so `0.1 + 0.2` renders as `0.3` not `0.30000000000000004`. Falls
 *  back to the raw `toString` for non-finite numbers (NaN, ±∞). */
function formatNumber(n: number): string {
    if (!Number.isFinite(n)) return String(n);
    // 10-digit precision swallows IEEE 754 noise for everyday math
    // without destroying legitimate fractional precision.
    const fixed = n.toFixed(10);
    return fixed.includes('.') ? fixed.replace(/\.?0+$/, '') : fixed;
}

/** Format a Date as locale-agnostic ISO YYYY-MM-DD in local time. */
function formatDate(d: Date): string {
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    return `${y}-${m}-${day}`;
}

/** Weekday lookups, Sunday = 0 (matches Date.getDay()). */
const WEEKDAY_KEYS = [
    'sunday',
    'monday',
    'tuesday',
    'wednesday',
    'thursday',
    'friday',
    'saturday',
];
const WEEKDAY_NAMES = [
    'Sunday',
    'Monday',
    'Tuesday',
    'Wednesday',
    'Thursday',
    'Friday',
    'Saturday',
];

/** Render a date as "YYYY-MM-DD (Weekday)" — the natural-language date
 *  evaluators show both the ISO date and the day name so "next Friday"
 *  confirms it actually landed on a Friday. */
function describeDate(d: Date): string {
    return `${formatDate(d)} (${WEEKDAY_NAMES[d.getDay()]})`;
}

/** Parse a number that may carry a base prefix (0x / 0b / 0o) or be
 *  plain decimal. Returns null for malformed input. */
function parseInteger(s: string): number | null {
    const lower = s.toLowerCase();
    if (lower.startsWith('0x')) {
        return /^[0-9a-f]+$/.test(lower.slice(2)) ? parseInt(lower.slice(2), 16) : null;
    }
    if (lower.startsWith('0b')) {
        return /^[01]+$/.test(lower.slice(2)) ? parseInt(lower.slice(2), 2) : null;
    }
    if (lower.startsWith('0o')) {
        return /^[0-7]+$/.test(lower.slice(2)) ? parseInt(lower.slice(2), 8) : null;
    }
    if (/^-?\d+$/.test(s)) return parseInt(s, 10);
    return null;
}

// ─── Percentage ──────────────────────────────────────────────────────

/**
 *   "20% of 80"           → 16            (calculator)
 *   "what is 15% of 200"  → 30            (calculator)
 *   "10 is what % of 50"  → 20%           (calculator)
 *   "120 increase by 25%" → 150           (calculator)
 *   "120 decrease by 25%" → 90            (calculator)
 *   "120 plus 25%"        → 150           (alias)
 *   "120 minus 25%"       → 90            (alias)
 */
function evalPercentage(value: string): RenderableQuickAction | null {
    const v = value.trim();
    const ofMatch = v.match(
        /^(?:what\s+is\s+)?([0-9]*\.?[0-9]+)\s*%\s+of\s+([0-9]*\.?[0-9]+)\s*$/i,
    );
    if (ofMatch) {
        const pct = parseFloat(ofMatch[1]);
        const base = parseFloat(ofMatch[2]);
        return {
            type: 'calculator',
            expression: `${formatNumber(pct)}% of ${formatNumber(base)}`,
            result: formatNumber((pct / 100) * base),
        };
    }
    const whatPctMatch = v.match(
        /^([0-9]*\.?[0-9]+)\s+is\s+what\s+%\s+of\s+([0-9]*\.?[0-9]+)\s*$/i,
    );
    if (whatPctMatch) {
        const part = parseFloat(whatPctMatch[1]);
        const whole = parseFloat(whatPctMatch[2]);
        if (whole === 0) return null;
        return {
            type: 'calculator',
            expression: `${formatNumber(part)} is what % of ${formatNumber(whole)}`,
            result: `${formatNumber((part / whole) * 100)}%`,
        };
    }
    const adjMatch = v.match(
        /^([0-9]*\.?[0-9]+)\s+(increase|decrease|plus|minus)(?:\s+by)?\s+([0-9]*\.?[0-9]+)\s*%\s*$/i,
    );
    if (adjMatch) {
        const base = parseFloat(adjMatch[1]);
        const op = adjMatch[2].toLowerCase();
        const pct = parseFloat(adjMatch[3]);
        const delta = (base * pct) / 100;
        const isAdd = op === 'increase' || op === 'plus';
        return {
            type: 'calculator',
            expression: `${formatNumber(base)} ${isAdd ? '+' : '-'} ${formatNumber(pct)}%`,
            result: formatNumber(isAdd ? base + delta : base - delta),
        };
    }
    return null;
}

// ─── Base conversion (hex / dec / bin / oct) ─────────────────────────

/**
 *   "0xff to dec"     → 255
 *   "255 to hex"      → 0xff
 *   "0b1010 to dec"   → 10
 *   "10 in binary"    → 0b1010
 *   "0o755 to dec"    → 493
 */
function evalBaseConversion(value: string): RenderableQuickAction | null {
    const v = value.trim();
    const match = v.match(
        /^(0x[0-9a-f]+|0b[01]+|0o[0-7]+|[0-9]+)\s+(?:to|in|as)\s+(dec(?:imal)?|hex(?:adecimal)?|bin(?:ary)?|oct(?:al)?)\s*$/i,
    );
    if (!match) return null;
    const input = match[1];
    const targetRaw = match[2].toLowerCase();
    const target = targetRaw.startsWith('dec')
        ? 'dec'
        : targetRaw.startsWith('hex')
          ? 'hex'
          : targetRaw.startsWith('bin')
            ? 'bin'
            : 'oct';
    const n = parseInteger(input);
    if (n === null || !Number.isFinite(n)) return null;
    const result =
        target === 'dec'
            ? String(n)
            : target === 'hex'
              ? `0x${n.toString(16)}`
              : target === 'bin'
                ? `0b${n.toString(2)}`
                : `0o${n.toString(8)}`;
    return { type: 'unitConversion', original: input, result };
}

// ─── Date math ───────────────────────────────────────────────────────

/**
 *   "3 days from now"        → YYYY-MM-DD
 *   "2 weeks from now"       → YYYY-MM-DD
 *   "5 days ago"             → YYYY-MM-DD
 *   "days between A and B"   → N days
 */
function evalDateMath(value: string): RenderableQuickAction | null {
    const v = value.trim();
    const offsetMatch = v.match(
        /^([0-9]+)\s+(day|days|week|weeks|month|months|year|years)\s+(from\s+now|ago)\s*$/i,
    );
    if (offsetMatch) {
        const amount = parseInt(offsetMatch[1], 10);
        const unit = offsetMatch[2].toLowerCase();
        const direction = offsetMatch[3].toLowerCase().includes('ago') ? -1 : 1;
        const d = new Date();
        const signed = amount * direction;
        if (unit.startsWith('day')) d.setDate(d.getDate() + signed);
        else if (unit.startsWith('week')) d.setDate(d.getDate() + signed * 7);
        else if (unit.startsWith('month')) d.setMonth(d.getMonth() + signed);
        else if (unit.startsWith('year')) d.setFullYear(d.getFullYear() + signed);
        return { type: 'unitConversion', original: v, result: formatDate(d) };
    }
    const betweenMatch = v.match(
        /^days\s+between\s+(\d{4}-\d{2}-\d{2})\s+and\s+(\d{4}-\d{2}-\d{2})\s*$/i,
    );
    if (betweenMatch) {
        const a = new Date(betweenMatch[1] + 'T00:00:00');
        const b = new Date(betweenMatch[2] + 'T00:00:00');
        if (Number.isNaN(a.getTime()) || Number.isNaN(b.getTime())) return null;
        const days = Math.round(Math.abs(b.getTime() - a.getTime()) / 86_400_000);
        return {
            type: 'calculator',
            expression: `days between ${betweenMatch[1]} and ${betweenMatch[2]}`,
            result: `${days} day${days === 1 ? '' : 's'}`,
        };
    }
    return null;
}

// ─── Natural-language dates ──────────────────────────────────────────

/**
 * Conversational date phrases that resolve to a concrete date.
 *
 *   "today"          → 2026-05-23 (Saturday)
 *   "tomorrow"       → 2026-05-24 (Sunday)
 *   "yesterday"      → 2026-05-22 (Friday)
 *   "next friday"    → next upcoming Friday (never today)
 *   "friday"         → bare weekday = next upcoming occurrence
 *   "last monday"    → most recent past Monday (never today)
 *   "this saturday"  → the occurrence in the current Sun–Sat week
 *   "in 3 days"      → today + 3 days   (alias for "3 days from now")
 *   "in 2 weeks"     → today + 14 days
 *   "next week"      → +7 days · "next month" → +1 month · "next year" → +1 year
 *   "last week/month/year" → the inverse
 *
 * Distinct in shape from `evalDateMath` (which is numeric "N <unit> from
 * now / ago" + "days between …"), so the two never collide.
 */
function evalNaturalDate(value: string): RenderableQuickAction | null {
    const v = value.trim().toLowerCase();
    if (!v) return null;

    // today / tomorrow / yesterday
    if (v === 'today') {
        return { type: 'unitConversion', original: 'today', result: describeDate(new Date()) };
    }
    if (v === 'tomorrow') {
        const d = new Date();
        d.setDate(d.getDate() + 1);
        return { type: 'unitConversion', original: 'tomorrow', result: describeDate(d) };
    }
    if (v === 'yesterday') {
        const d = new Date();
        d.setDate(d.getDate() - 1);
        return { type: 'unitConversion', original: 'yesterday', result: describeDate(d) };
    }

    // next / last  week | month | year
    const periodMatch = v.match(/^(next|last)\s+(week|month|year)$/);
    if (periodMatch) {
        const dir = periodMatch[1] === 'last' ? -1 : 1;
        const d = new Date();
        if (periodMatch[2] === 'week') d.setDate(d.getDate() + dir * 7);
        else if (periodMatch[2] === 'month') d.setMonth(d.getMonth() + dir);
        else d.setFullYear(d.getFullYear() + dir);
        return { type: 'unitConversion', original: v, result: describeDate(d) };
    }

    // in N days | weeks | months | years
    const inMatch = v.match(/^in\s+(\d+)\s+(day|days|week|weeks|month|months|year|years)$/);
    if (inMatch) {
        const amount = parseInt(inMatch[1], 10);
        if (amount > 36500) return null; // sanity bound (~100 years)
        const unit = inMatch[2];
        const d = new Date();
        if (unit.startsWith('day')) d.setDate(d.getDate() + amount);
        else if (unit.startsWith('week')) d.setDate(d.getDate() + amount * 7);
        else if (unit.startsWith('month')) d.setMonth(d.getMonth() + amount);
        else d.setFullYear(d.getFullYear() + amount);
        return { type: 'unitConversion', original: v, result: describeDate(d) };
    }

    // [next | last | this] <weekday>   — bare weekday = next occurrence
    const wdMatch = v.match(
        /^(?:(next|last|this)\s+)?(sunday|monday|tuesday|wednesday|thursday|friday|saturday)$/,
    );
    if (wdMatch) {
        const qualifier = wdMatch[1]; // undefined for a bare weekday
        const targetDow = WEEKDAY_KEYS.indexOf(wdMatch[2]);
        const today = new Date();
        const todayDow = today.getDay();
        const d = new Date(today);
        if (qualifier === 'last') {
            // most recent PAST occurrence (never today)
            let diff = (todayDow - targetDow + 7) % 7;
            if (diff === 0) diff = 7;
            d.setDate(today.getDate() - diff);
        } else if (qualifier === 'this') {
            // occurrence within the current Sun–Sat week (may be in the past)
            d.setDate(today.getDate() + (targetDow - todayDow));
        } else {
            // 'next' or bare → next upcoming occurrence (never today)
            let diff = (targetDow - todayDow + 7) % 7;
            if (diff === 0) diff = 7;
            d.setDate(today.getDate() + diff);
        }
        return { type: 'unitConversion', original: v, result: describeDate(d) };
    }

    return null;
}

// ─── Day of week ─────────────────────────────────────────────────────

/**
 *   "2026-12-25 day"       → Friday
 *   "2026-12-25 weekday"   → Friday
 */
function evalDayOfWeek(value: string): RenderableQuickAction | null {
    const m = value.trim().match(/^(\d{4}-\d{2}-\d{2})\s+(?:day(?:\s+of\s+week)?|weekday)\s*$/i);
    if (!m) return null;
    const d = new Date(m[1] + 'T00:00:00');
    if (Number.isNaN(d.getTime())) return null;
    const names = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
    return { type: 'unitConversion', original: m[1], result: names[d.getDay()] };
}

// ─── Age from birthdate ──────────────────────────────────────────────

/** "age from 1990-05-15" → "36y 6d" (years + days since last birthday) */
function evalAge(value: string): RenderableQuickAction | null {
    const m = value.trim().match(/^age\s+from\s+(\d{4}-\d{2}-\d{2})\s*$/i);
    if (!m) return null;
    const birth = new Date(m[1] + 'T00:00:00');
    if (Number.isNaN(birth.getTime())) return null;
    const now = new Date();
    let years = now.getFullYear() - birth.getFullYear();
    const monthDiff = now.getMonth() - birth.getMonth();
    if (monthDiff < 0 || (monthDiff === 0 && now.getDate() < birth.getDate())) {
        years--;
    }
    // Days since most recent birthday (zero on the birthday itself).
    const lastBday = new Date(birth);
    lastBday.setFullYear(birth.getFullYear() + years);
    const days = Math.floor((now.getTime() - lastBday.getTime()) / 86_400_000);
    return {
        type: 'calculator',
        expression: `age from ${m[1]}`,
        result: `${years}y ${days}d`,
    };
}

// ─── Workdays (skip Sat/Sun) ─────────────────────────────────────────

/** "5 workdays from now" → next-business-day date YYYY-MM-DD */
function evalWorkdays(value: string): RenderableQuickAction | null {
    const m = value.trim().match(/^(\d+)\s+workdays?\s+from\s+now\s*$/i);
    if (!m) return null;
    const target = parseInt(m[1], 10);
    if (target < 0 || target > 365 * 5) return null; // bound
    const d = new Date();
    let added = 0;
    while (added < target) {
        d.setDate(d.getDate() + 1);
        const dow = d.getDay();
        if (dow !== 0 && dow !== 6) added++;
    }
    return { type: 'unitConversion', original: m[0], result: formatDate(d) };
}

// ─── Time arithmetic (clock + duration) ──────────────────────────────

/** Parse "9:30", "9:30am", "9pm", "17:30" → minutes since midnight, or null. */
function parseTime(s: string): number | null {
    const m = s.trim().match(/^(\d{1,2})(?::(\d{2}))?\s*(am|pm)?\s*$/i);
    if (!m) return null;
    let h = parseInt(m[1], 10);
    const min = m[2] ? parseInt(m[2], 10) : 0;
    const ampm = m[3]?.toLowerCase();
    if (ampm === 'pm' && h < 12) h += 12;
    if (ampm === 'am' && h === 12) h = 0;
    if (h < 0 || h > 23 || min < 0 || min > 59) return null;
    return h * 60 + min;
}

/** Parse "2h 30m", "90 min", "1.5 hours" → total minutes, or null. */
function parseDuration(s: string): number | null {
    let total = 0;
    let matched = false;
    const hMatch = s.match(/(\d+(?:\.\d+)?)\s*(?:h|hr|hrs|hour|hours)\b/i);
    if (hMatch) {
        total += parseFloat(hMatch[1]) * 60;
        matched = true;
    }
    const mMatch = s.match(/(\d+(?:\.\d+)?)\s*(?:m|min|mins|minute|minutes)\b/i);
    if (mMatch) {
        total += parseFloat(mMatch[1]);
        matched = true;
    }
    return matched ? total : null;
}

/** "9:30 + 2h 30m" → 12:00 (wraps within 24h) */
function evalTimeArithmetic(value: string): RenderableQuickAction | null {
    const v = value.trim();
    const m = v.match(/^(\d{1,2}(?::\d{2})?\s*(?:am|pm)?)\s*([+\-])\s*(.+?)\s*$/i);
    if (!m) return null;
    const base = parseTime(m[1]);
    if (base === null) return null;
    const sign = m[2] === '+' ? 1 : -1;
    const dur = parseDuration(m[3]);
    if (dur === null) return null;
    let total = base + sign * dur;
    total = ((total % (24 * 60)) + 24 * 60) % (24 * 60); // wrap into [0, 1440)
    const outH = Math.floor(total / 60);
    const outM = total % 60;
    return {
        type: 'calculator',
        expression: v,
        result: `${String(outH).padStart(2, '0')}:${String(outM).padStart(2, '0')}`,
    };
}

/** "from 9am to 5:30pm" → 8h 30m (handles crossing midnight) */
function evalTimeRange(value: string): RenderableQuickAction | null {
    const v = value.trim();
    const m = v.match(/^from\s+(.+?)\s+to\s+(.+?)\s*$/i);
    if (!m) return null;
    const a = parseTime(m[1]);
    const b = parseTime(m[2]);
    if (a === null || b === null) return null;
    let diff = b - a;
    if (diff < 0) diff += 24 * 60; // overnight
    const h = Math.floor(diff / 60);
    const min = diff % 60;
    let result = '';
    if (h > 0) result += `${h}h`;
    if (min > 0) result += (result ? ' ' : '') + `${min}m`;
    if (!result) result = '0m';
    return { type: 'calculator', expression: v, result };
}

// ─── Color conversion ────────────────────────────────────────────────

type RGB = { r: number; g: number; b: number };

/** Parse hex (#rgb or #rrggbb), rgb(...), or hsl(...) → RGB or null. */
function parseColor(s: string): RGB | null {
    const t = s.trim();
    const hex = t.match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
    if (hex) {
        const h = hex[1];
        if (h.length === 3) {
            return {
                r: parseInt(h[0] + h[0], 16),
                g: parseInt(h[1] + h[1], 16),
                b: parseInt(h[2] + h[2], 16),
            };
        }
        return {
            r: parseInt(h.slice(0, 2), 16),
            g: parseInt(h.slice(2, 4), 16),
            b: parseInt(h.slice(4, 6), 16),
        };
    }
    const rgb = t.match(/^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i);
    if (rgb) {
        return {
            r: clampByte(parseInt(rgb[1], 10)),
            g: clampByte(parseInt(rgb[2], 10)),
            b: clampByte(parseInt(rgb[3], 10)),
        };
    }
    const hsl = t.match(/^hsla?\(\s*(\d+(?:\.\d+)?)\s*,\s*(\d+(?:\.\d+)?)%\s*,\s*(\d+(?:\.\d+)?)%/i);
    if (hsl) {
        return hslToRgb(parseFloat(hsl[1]), parseFloat(hsl[2]), parseFloat(hsl[3]));
    }
    return null;
}

function clampByte(n: number): number {
    return Math.max(0, Math.min(255, n));
}

function hslToRgb(h: number, s: number, l: number): RGB {
    s /= 100;
    l /= 100;
    const c = (1 - Math.abs(2 * l - 1)) * s;
    const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
    const mAdj = l - c / 2;
    let [r1, g1, b1] = [0, 0, 0];
    if (h < 60) [r1, g1, b1] = [c, x, 0];
    else if (h < 120) [r1, g1, b1] = [x, c, 0];
    else if (h < 180) [r1, g1, b1] = [0, c, x];
    else if (h < 240) [r1, g1, b1] = [0, x, c];
    else if (h < 300) [r1, g1, b1] = [x, 0, c];
    else [r1, g1, b1] = [c, 0, x];
    return {
        r: Math.round((r1 + mAdj) * 255),
        g: Math.round((g1 + mAdj) * 255),
        b: Math.round((b1 + mAdj) * 255),
    };
}

function rgbToHex(c: RGB): string {
    const h = (n: number) => n.toString(16).padStart(2, '0');
    return `#${h(c.r)}${h(c.g)}${h(c.b)}`;
}

function rgbToHsl(c: RGB): string {
    const r = c.r / 255;
    const g = c.g / 255;
    const b = c.b / 255;
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const d = max - min;
    let h = 0;
    const l = (max + min) / 2;
    const s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
    if (d !== 0) {
        if (max === r) h = 60 * (((g - b) / d) % 6);
        else if (max === g) h = 60 * ((b - r) / d + 2);
        else h = 60 * ((r - g) / d + 4);
    }
    if (h < 0) h += 360;
    return `hsl(${Math.round(h)}, ${Math.round(s * 100)}%, ${Math.round(l * 100)}%)`;
}

/** "#ff5500 to rgb" / "rgb(255,85,0) to hex" / "hsl(...) to hex" */
function evalColorConversion(value: string): RenderableQuickAction | null {
    const v = value.trim();
    const m = v.match(/^(.+?)\s+to\s+(hex|rgb|hsl)\s*$/i);
    if (!m) return null;
    const color = parseColor(m[1]);
    if (!color) return null;
    const target = m[2].toLowerCase();
    const result =
        target === 'hex'
            ? rgbToHex(color)
            : target === 'rgb'
              ? `rgb(${color.r}, ${color.g}, ${color.b})`
              : rgbToHsl(color);
    return { type: 'unitConversion', original: m[1], result };
}

/** WCAG 2.x relative luminance. */
function relativeLuminance(c: RGB): number {
    const lin = (v: number) => {
        const x = v / 255;
        return x <= 0.03928 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b);
}

/** "#fff vs #000 contrast" → "21:1 (AAA)" */
function evalColorContrast(value: string): RenderableQuickAction | null {
    const m = value.trim().match(/^(\S+)\s+vs\s+(\S+)\s+contrast\s*$/i);
    if (!m) return null;
    const a = parseColor(m[1]);
    const b = parseColor(m[2]);
    if (!a || !b) return null;
    const la = relativeLuminance(a);
    const lb = relativeLuminance(b);
    const ratio = (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
    const rounded = Math.round(ratio * 100) / 100;
    // WCAG conformance labels — normal-text thresholds.
    const grade = rounded >= 7 ? 'AAA' : rounded >= 4.5 ? 'AA' : rounded >= 3 ? 'AA Large' : 'fail';
    return {
        type: 'calculator',
        expression: `${m[1]} vs ${m[2]} contrast`,
        result: `${rounded}:1 (${grade})`,
    };
}

/** "random color" → random #rrggbb */
function evalRandomColor(value: string): RenderableQuickAction | null {
    if (!/^random\s+colou?r\s*$/i.test(value.trim())) return null;
    const rand = () => Math.floor(Math.random() * 256);
    const c = { r: rand(), g: rand(), b: rand() };
    return { type: 'unitConversion', original: 'random color', result: rgbToHex(c) };
}

// ─── Bitwise operations ──────────────────────────────────────────────

/**
 *   "0xff & 0x0f"   → 15
 *   "0xff | 0x0f"   → 255
 *   "0xff ^ 0xff"   → 0
 *   "~0xff"         → 0xffffff00 (unsigned 32-bit NOT)
 *   "0xff << 2"     → 0x3fc
 *   "0xff >> 2"     → 0x3f
 *
 * Results are formatted as "decimal (hex)" so the user gets both
 * representations without typing a follow-up conversion query.
 */
function evalBitwise(value: string): RenderableQuickAction | null {
    const v = value.trim();
    // Unary NOT (~x or "not x")
    const notMatch = v.match(/^(?:~|not\s+)(\S+)\s*$/i);
    if (notMatch) {
        const n = parseInteger(notMatch[1]);
        if (n === null) return null;
        const result = ~n >>> 0;
        return {
            type: 'calculator',
            expression: `~${notMatch[1]}`,
            result: `${result} (0x${result.toString(16)})`,
        };
    }
    // Binary ops: &, |, ^, <<, >>, plus word aliases
    const binMatch = v.match(
        /^(\S+)\s+(and|or|xor|<<|>>|&|\||\^)\s+(\S+)\s*$/i,
    );
    if (binMatch) {
        const a = parseInteger(binMatch[1]);
        const b = parseInteger(binMatch[3]);
        if (a === null || b === null) return null;
        let op = binMatch[2].toLowerCase();
        if (op === 'and') op = '&';
        if (op === 'or') op = '|';
        if (op === 'xor') op = '^';
        let result: number;
        switch (op) {
            case '&':
                result = a & b;
                break;
            case '|':
                result = a | b;
                break;
            case '^':
                result = a ^ b;
                break;
            case '<<':
                result = a << b;
                break;
            case '>>':
                result = a >> b;
                break;
            default:
                return null;
        }
        return {
            type: 'calculator',
            expression: `${binMatch[1]} ${op} ${binMatch[3]}`,
            result: `${result} (0x${(result >>> 0).toString(16)})`,
        };
    }
    return null;
}

// ─── String operations ───────────────────────────────────────────────

/**
 *   length of "hello"       → 5 chars
 *   words in "..."          → N words
 *   reverse "abc"           → cba
 *   base64 "hi"             → aGk=
 *   unbase64 "aGk="         → hi
 *   urlencode "a b"         → a%20b
 *   urldecode "a%20b"       → a b
 */
function evalStringOps(value: string): RenderableQuickAction | null {
    const v = value.trim();

    const lenMatch = v.match(/^length\s+of\s+"([^"]*)"\s*$/i);
    if (lenMatch) {
        // Array.from counts code points (so emojis count as 1), not UTF-16
        // code units — closer to what a user means by "length".
        const len = Array.from(lenMatch[1]).length;
        return {
            type: 'calculator',
            expression: `length of "${lenMatch[1]}"`,
            result: `${len} char${len === 1 ? '' : 's'}`,
        };
    }

    const wordsMatch = v.match(/^words\s+in\s+"([^"]*)"\s*$/i);
    if (wordsMatch) {
        const w = wordsMatch[1].trim().split(/\s+/).filter(Boolean).length;
        return {
            type: 'calculator',
            expression: `words in "${wordsMatch[1]}"`,
            result: `${w} word${w === 1 ? '' : 's'}`,
        };
    }

    const revMatch = v.match(/^reverse\s+"([^"]*)"\s*$/i);
    if (revMatch) {
        // Code-point-aware reverse so multi-byte chars stay intact.
        const reversed = Array.from(revMatch[1]).reverse().join('');
        return {
            type: 'unitConversion',
            original: `"${revMatch[1]}"`,
            result: reversed || '(empty)',
        };
    }

    const b64Match = v.match(/^base64\s+"([^"]*)"\s*$/i);
    if (b64Match) {
        try {
            // btoa requires Latin-1 — wrap in a TextEncoder roundtrip so
            // multi-byte chars are encoded as UTF-8 first, matching what
            // a developer expects from a "base64 this string" command.
            const bytes = new TextEncoder().encode(b64Match[1]);
            let bin = '';
            for (const byte of bytes) bin += String.fromCharCode(byte);
            return {
                type: 'unitConversion',
                original: `"${b64Match[1]}"`,
                result: btoa(bin),
            };
        } catch {
            return null;
        }
    }

    const ub64Match = v.match(/^(?:unbase64|debase64|decode\s+base64)\s+"([^"]*)"\s*$/i);
    if (ub64Match) {
        try {
            const bin = atob(ub64Match[1]);
            const bytes = new Uint8Array(bin.length);
            for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
            const decoded = new TextDecoder().decode(bytes);
            return {
                type: 'unitConversion',
                original: `"${ub64Match[1]}"`,
                result: decoded || '(empty)',
            };
        } catch {
            return null;
        }
    }

    const encMatch = v.match(/^(?:urlencode|encodeuri(?:component)?|url\s+encode)\s+"([^"]*)"\s*$/i);
    if (encMatch) {
        return {
            type: 'unitConversion',
            original: `"${encMatch[1]}"`,
            result: encodeURIComponent(encMatch[1]),
        };
    }

    const decMatch = v.match(/^(?:urldecode|decodeuri(?:component)?|url\s+decode)\s+"([^"]*)"\s*$/i);
    if (decMatch) {
        try {
            return {
                type: 'unitConversion',
                original: `"${decMatch[1]}"`,
                result: decodeURIComponent(decMatch[1]),
            };
        } catch {
            return null;
        }
    }

    return null;
}

// ─── UUID + Random number ────────────────────────────────────────────

/** "uuid" → fresh UUID v4 */
function evalUuid(value: string): RenderableQuickAction | null {
    if (!/^uuid\s*$/i.test(value.trim())) return null;
    const uuid =
        typeof crypto !== 'undefined' && 'randomUUID' in crypto
            ? // crypto.randomUUID is the modern path — cryptographically random
              (crypto as Crypto & { randomUUID: () => string }).randomUUID()
            : // Fallback for older engines — Math.random is good enough for a
              // throwaway identifier in this context.
              'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
                  const r = (Math.random() * 16) | 0;
                  return (c === 'x' ? r : (r & 0x3) | 0x8).toString(16);
              });
    return { type: 'unitConversion', original: 'uuid', result: uuid };
}

/** "random 1-100" → integer in [1, 100] inclusive */
function evalRandomRange(value: string): RenderableQuickAction | null {
    const m = value.trim().match(/^random\s+(-?\d+)\s*-\s*(-?\d+)\s*$/i);
    if (!m) return null;
    const a = parseInt(m[1], 10);
    const b = parseInt(m[2], 10);
    const lo = Math.min(a, b);
    const hi = Math.max(a, b);
    const result = Math.floor(Math.random() * (hi - lo + 1)) + lo;
    return {
        type: 'calculator',
        expression: `random ${lo}-${hi}`,
        result: String(result),
    };
}

// ─── Tip / Discount ──────────────────────────────────────────────────

/** "tip 15% on $50" → "$7.50 (total $57.50)" */
function evalTipCalc(value: string): RenderableQuickAction | null {
    const m = value.trim().match(
        /^tip\s+(\d+(?:\.\d+)?)\s*%\s+on\s+\$?(\d+(?:\.\d+)?)\s*$/i,
    );
    if (!m) return null;
    const pct = parseFloat(m[1]);
    const base = parseFloat(m[2]);
    const tip = (base * pct) / 100;
    const total = base + tip;
    return {
        type: 'calculator',
        expression: `tip ${formatNumber(pct)}% on $${formatNumber(base)}`,
        result: `$${tip.toFixed(2)} (total $${total.toFixed(2)})`,
    };
}

/** "30% off $100" → "$70.00 (saved $30.00)" */
function evalDiscount(value: string): RenderableQuickAction | null {
    const m = value.trim().match(
        /^(\d+(?:\.\d+)?)\s*%\s+off\s+\$?(\d+(?:\.\d+)?)\s*$/i,
    );
    if (!m) return null;
    const pct = parseFloat(m[1]);
    const base = parseFloat(m[2]);
    const discount = (base * pct) / 100;
    const final = base - discount;
    return {
        type: 'calculator',
        expression: `${formatNumber(pct)}% off $${formatNumber(base)}`,
        result: `$${final.toFixed(2)} (saved $${discount.toFixed(2)})`,
    };
}

// ─── Quick stats over a list ─────────────────────────────────────────

/**
 *   sum 1,2,3,4,5      → 15
 *   avg 1 2 3 4 5      → 3
 *   min 5,3,8,1        → 1
 *   max 5,3,8,1        → 8
 *   count 1,2,3        → 3
 *   product 2,3,4      → 24
 */
function evalQuickStats(value: string): RenderableQuickAction | null {
    const m = value.trim().match(
        /^(sum|avg|average|mean|min|max|count|product)\s+(.+)$/i,
    );
    if (!m) return null;
    const op = m[1].toLowerCase();
    const numbers = m[2]
        .split(/[,\s]+/)
        .map((s) => parseFloat(s))
        .filter((n) => !Number.isNaN(n));
    if (numbers.length === 0) return null;
    let result: number;
    switch (op) {
        case 'sum':
            result = numbers.reduce((a, b) => a + b, 0);
            break;
        case 'avg':
        case 'average':
        case 'mean':
            result = numbers.reduce((a, b) => a + b, 0) / numbers.length;
            break;
        case 'min':
            result = Math.min(...numbers);
            break;
        case 'max':
            result = Math.max(...numbers);
            break;
        case 'count':
            result = numbers.length;
            break;
        case 'product':
            result = numbers.reduce((a, b) => a * b, 1);
            break;
        default:
            return null;
    }
    return {
        type: 'calculator',
        expression: `${op} ${numbers.join(', ')}`,
        result: formatNumber(result),
    };
}

// ─── Advanced math ───────────────────────────────────────────────────

/**
 * Trig, log, sqrt, powers, factorials, constants, modulo.
 *
 *   sin(45deg)    → 0.7071...
 *   cos(pi/4)     → 0.7071...
 *   log(100)      → 2          (base-10 log, Raycast convention)
 *   ln(e)         → 1
 *   log2(256)     → 8
 *   sqrt(16)      → 4
 *   2^10          → 1024
 *   5!            → 120
 *   17 mod 5      → 2
 *   pi            → 3.14159265...
 *
 * Implementation: regex-restricted input → JS expression → `new
 * Function` eval. Two layers of safety:
 *   1. Allowlist of characters at the entry — only letters, digits,
 *      arithmetic operators, parens, dot, comma, whitespace, degree
 *      sign, factorial. No quotes, brackets, semicolons, etc.
 *   2. Identifier allowlist after transformation — every name token
 *      surviving the function/constant substitutions must be in the
 *      SAFE_GLOBALS set. Catches anything like `Math.constructor`
 *      that the entry regex would otherwise allow through.
 *
 * Functions map: `log` → `Math.log10` (base-10, matches Raycast),
 * `ln` → `Math.log` (natural). All other names go through unchanged
 * (sin → Math.sin, etc.).
 */
const ADVANCED_FUNC_MAP: Record<string, string> = {
    sin: 'Math.sin',
    cos: 'Math.cos',
    tan: 'Math.tan',
    asin: 'Math.asin',
    acos: 'Math.acos',
    atan: 'Math.atan',
    sinh: 'Math.sinh',
    cosh: 'Math.cosh',
    tanh: 'Math.tanh',
    log: 'Math.log10',
    ln: 'Math.log',
    log2: 'Math.log2',
    log10: 'Math.log10',
    exp: 'Math.exp',
    sqrt: 'Math.sqrt',
    cbrt: 'Math.cbrt',
    abs: 'Math.abs',
    floor: 'Math.floor',
    ceil: 'Math.ceil',
    round: 'Math.round',
};

const ADVANCED_CONST_MAP: Record<string, number> = {
    pi: Math.PI,
    tau: 2 * Math.PI,
    phi: (1 + Math.sqrt(5)) / 2,
};

// After all substitutions, ONLY these identifiers may appear in the
// expression. Anything else trips the bail-out.
const SAFE_GLOBALS: ReadonlySet<string> = new Set([
    'Math',
    'sin',
    'cos',
    'tan',
    'asin',
    'acos',
    'atan',
    'sinh',
    'cosh',
    'tanh',
    'log',
    'log2',
    'log10',
    'ln',
    'exp',
    'sqrt',
    'cbrt',
    'abs',
    'floor',
    'ceil',
    'round',
    'PI',
    'E',
    'NaN',
    'Infinity',
]);

function evalAdvancedMath(value: string): RenderableQuickAction | null {
    const v = value.trim();
    if (!v) return null;

    // Entry allowlist — must be made of only the chars below. Any other
    // character (quotes, brackets, semicolons, etc.) bails out before
    // we even consider transformation.
    if (!/^[a-z0-9.+\-*/^!%()\s,°]+$/i.test(v)) return null;

    // Must contain at least one "advanced" signal — otherwise it's
    // plain arithmetic and the backend handles it fine.
    const hasAdvancedSignal =
        /\b(sin|cos|tan|asin|acos|atan|sinh|cosh|tanh|log|ln|log2|log10|exp|sqrt|cbrt|abs|floor|ceil|round|pi|tau|phi|mod)\b/i.test(
            v,
        ) ||
        v.includes('^') ||
        /\d+\s*!/.test(v) ||
        // Bare constant (`pi`, `e`, `tau`, `phi`) → return its value.
        /^(?:pi|e|tau|phi)\s*$/i.test(v);
    if (!hasAdvancedSignal) return null;

    try {
        let expr = v;

        // `^` → `**` (JS power operator).
        expr = expr.replace(/\^/g, '**');

        // `mod` keyword → `%` operator.
        expr = expr.replace(/\bmod\b/gi, '%');

        // Factorial — replace `N!` with the computed integer. Bounded so
        // we never compute 1000! (which would just be Infinity anyway).
        expr = expr.replace(/(\d+)\s*!/g, (_match, n: string) => {
            const num = parseInt(n, 10);
            if (num < 0 || num > 170) return 'NaN';
            let f = 1;
            for (let i = 2; i <= num; i++) f *= i;
            return String(f);
        });

        // Degree suffix — `45deg` or `45°` → `(45 * Math.PI / 180)`.
        expr = expr.replace(/([\d.]+)\s*(deg|°)\b/gi, '($1 * Math.PI / 180)');

        // Constants — `pi`, `tau`, `phi` replaced with their numeric
        // values. Done before function substitution so we don't acci-
        // dentally match `pi` inside a longer identifier.
        for (const [name, val] of Object.entries(ADVANCED_CONST_MAP)) {
            expr = expr.replace(new RegExp(`\\b${name}\\b`, 'g'), `(${val})`);
        }

        // Functions — sort longest-first so `log10` is replaced before
        // `log` (otherwise the substring match wrecks the longer name).
        const fnNames = Object.keys(ADVANCED_FUNC_MAP).sort(
            (a, b) => b.length - a.length,
        );
        for (const fn of fnNames) {
            expr = expr.replace(new RegExp(`\\b${fn}\\b`, 'g'), ADVANCED_FUNC_MAP[fn]);
        }

        // `e` is Math.E only when isolated — not when it's part of
        // 'exp', 'Math.exp(', etc. The earlier function substitution
        // already turned `exp(...)` into `Math.exp(...)`, so a remaining
        // bare `e` is safe to replace.
        expr = expr.replace(/\be\b/g, `(${Math.E})`);

        // Identifier allowlist — scan everything that LOOKS like a name
        // and require it to be in SAFE_GLOBALS. Catches malicious input
        // that survived the entry regex (e.g. `Math.constructor`).
        const idents = expr.match(/[a-zA-Z_$][a-zA-Z0-9_$]*/g) ?? [];
        for (const id of idents) {
            if (!SAFE_GLOBALS.has(id)) return null;
        }

        // Final eval. `"use strict"` so `with` and other sloppy-mode
        // foot-guns are off. Input is doubly-vetted; the worst case
        // is a thrown SyntaxError which the catch swallows.
        const result = new Function(`"use strict"; return (${expr});`)();
        if (typeof result !== 'number' || !Number.isFinite(result)) return null;
        return {
            type: 'calculator',
            expression: v,
            result: formatNumber(result),
        };
    } catch {
        return null;
    }
}
