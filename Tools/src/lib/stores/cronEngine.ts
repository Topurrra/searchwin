/*
  cronEngine — PURE cron parser, explainer, and run-time projector for the
  Cron Builder. Standard Vixie/POSIX cron: 5 fields (min hour dom month dow),
  an optional leading seconds field (6 fields), and the @macro shorthands.
  No DOM / Svelte deps → fully unit-testable (see cronEngine.test.ts).

  Day semantics follow crontab(5): when BOTH day-of-month and day-of-week are
  restricted (neither is '*'), a day matches if EITHER field matches (union).
*/

export class CronError extends Error {}

export interface CronField {
    raw: string; // original text for this field
    values: number[]; // sorted, unique, in-range allowed values
    isWildcard: boolean; // raw === '*'
    wholeStep?: number; // n when the field is exactly '*/n' (for nice wording)
}

export interface ParsedCron {
    hasSeconds: boolean;
    seconds: CronField; // {0} for 5-field expressions
    minute: CronField;
    hour: CronField;
    dayOfMonth: CronField;
    month: CronField;
    dayOfWeek: CronField;
    reboot: boolean; // @reboot — not time-based, has no next runs
    normalized: string; // canonical field expression (post-macro)
}

interface FieldSpec {
    label: string;
    min: number;
    max: number;
    names?: string[]; // lowercase names, names[0] maps to `min`
    wrapSeven?: boolean; // day-of-week: 7 → 0 (both are Sunday)
}

const MONTHS = ['jan', 'feb', 'mar', 'apr', 'may', 'jun', 'jul', 'aug', 'sep', 'oct', 'nov', 'dec'];
const DOWS = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'];
const MONTH_FULL = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const DOW_FULL = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];

const SEC: FieldSpec = { label: 'second', min: 0, max: 59 };
const MIN: FieldSpec = { label: 'minute', min: 0, max: 59 };
const HOUR: FieldSpec = { label: 'hour', min: 0, max: 23 };
const DOM: FieldSpec = { label: 'day-of-month', min: 1, max: 31 };
const MONTH: FieldSpec = { label: 'month', min: 1, max: 12, names: MONTHS };
const DOW: FieldSpec = { label: 'day-of-week', min: 0, max: 6, names: DOWS, wrapSeven: true };

const MACROS: Record<string, string> = {
    '@yearly': '0 0 1 1 *',
    '@annually': '0 0 1 1 *',
    '@monthly': '0 0 1 * *',
    '@weekly': '0 0 * * 0',
    '@daily': '0 0 * * *',
    '@midnight': '0 0 * * *',
    '@hourly': '0 * * * *',
};

// ─── Parsing ─────────────────────────────────────────────────────────────────
function resolveName(s: string, spec: FieldSpec): number {
    const lower = s.toLowerCase();
    if (spec.names) {
        const idx = spec.names.indexOf(lower);
        if (idx >= 0) return idx + spec.min;
    }
    if (!/^\d+$/.test(s)) throw new CronError(`Invalid ${spec.label} value: "${s}"`);
    let n = Number(s);
    if (spec.wrapSeven && n === 7) n = 0;
    return n;
}

function inRange(n: number, spec: FieldSpec): boolean {
    return n >= spec.min && n <= spec.max;
}

/** Expand one comma-separated token: a wildcard, a step, a range, or a value. */
function parseToken(tok: string, spec: FieldSpec): number[] {
    if (!tok) throw new CronError(`Empty ${spec.label} value`);
    let base = tok;
    let step: number | undefined;
    const slash = tok.indexOf('/');
    if (slash >= 0) {
        base = tok.slice(0, slash);
        const stepStr = tok.slice(slash + 1);
        if (!/^\d+$/.test(stepStr)) throw new CronError(`Invalid step in "${tok}"`);
        step = Number(stepStr);
        if (step <= 0) throw new CronError(`Step must be greater than 0 in "${tok}"`);
    }

    let lo: number;
    let hi: number;
    if (base === '*') {
        lo = spec.min;
        hi = spec.max;
    } else if (base.includes('-')) {
        const [a, b] = base.split('-');
        lo = resolveName(a, spec);
        hi = resolveName(b, spec);
        if (!inRange(lo, spec) || !inRange(hi, spec)) throw new CronError(`${spec.label} out of range in "${tok}"`);
        if (lo > hi) throw new CronError(`Range start after end in "${tok}"`);
    } else {
        lo = resolveName(base, spec);
        if (!inRange(lo, spec)) throw new CronError(`${spec.label} out of range: "${base}"`);
        // "a/n" means a, a+n, … up to the field max; a bare "a" is just itself.
        hi = step !== undefined ? spec.max : lo;
    }

    const out: number[] = [];
    const s = step ?? 1;
    for (let v = lo; v <= hi; v += s) out.push(v);
    return out;
}

function parseField(raw: string, spec: FieldSpec): CronField {
    const trimmed = raw.trim();
    if (!trimmed) throw new CronError(`Missing ${spec.label} field`);
    const set = new Set<number>();
    for (const tok of trimmed.split(',')) for (const v of parseToken(tok.trim(), spec)) set.add(v);
    const values = [...set].sort((a, b) => a - b);
    const wholeMatch = /^\*\/(\d+)$/.exec(trimmed);
    return {
        raw: trimmed,
        values,
        isWildcard: trimmed === '*',
        wholeStep: wholeMatch ? Number(wholeMatch[1]) : undefined,
    };
}

/** Parse a cron expression (5 or 6 fields, or an @macro). Throws CronError. */
export function parseCron(expr: string): ParsedCron {
    const trimmed = expr.trim();
    if (!trimmed) throw new CronError('Empty expression');

    if (trimmed.toLowerCase() === '@reboot') {
        const zero = parseField('0', SEC);
        const star = parseField('*', MIN);
        return {
            hasSeconds: false,
            seconds: zero,
            minute: star,
            hour: parseField('*', HOUR),
            dayOfMonth: parseField('*', DOM),
            month: parseField('*', MONTH),
            dayOfWeek: parseField('*', DOW),
            reboot: true,
            normalized: '@reboot',
        };
    }

    const macro = MACROS[trimmed.toLowerCase()];
    const body = macro ?? trimmed;
    const parts = body.split(/\s+/);
    if (parts.length !== 5 && parts.length !== 6) {
        throw new CronError(`Expected 5 or 6 fields, got ${parts.length}`);
    }

    const hasSeconds = parts.length === 6;
    const [secRaw, ...rest] = hasSeconds ? parts : ['0', ...parts];
    const [minRaw, hourRaw, domRaw, monthRaw, dowRaw] = rest;

    return {
        hasSeconds,
        seconds: parseField(secRaw, SEC),
        minute: parseField(minRaw, MIN),
        hour: parseField(hourRaw, HOUR),
        dayOfMonth: parseField(domRaw, DOM),
        month: parseField(monthRaw, MONTH),
        dayOfWeek: parseField(dowRaw, DOW),
        reboot: false,
        normalized: body,
    };
}

/** Validate without throwing — returns an error message or null. */
export function validateCron(expr: string): string | null {
    try {
        parseCron(expr);
        return null;
    } catch (e) {
        return e instanceof Error ? e.message : String(e);
    }
}

// ─── Next runs ───────────────────────────────────────────────────────────────
function dayMatches(p: ParsedCron, d: Date): boolean {
    const domRestricted = !p.dayOfMonth.isWildcard;
    const dowRestricted = !p.dayOfWeek.isWildcard;
    const domOk = p.dayOfMonth.values.includes(d.getDate());
    const dowOk = p.dayOfWeek.values.includes(d.getDay());
    if (domRestricted && dowRestricted) return domOk || dowOk; // crontab(5) union
    if (domRestricted) return domOk;
    if (dowRestricted) return dowOk;
    return true;
}

/** Project the next `count` fire times at or after `from` (exclusive of `from`).
 *  Works in LOCAL time, matching Windows Task Scheduler triggers. */
export function nextRuns(p: ParsedCron, from: Date, count: number): Date[] {
    if (p.reboot || count <= 0) return [];
    const out: Date[] = [];
    const d = new Date(from.getTime());
    // Step off the current instant to the next candidate unit.
    if (p.hasSeconds) {
        d.setMilliseconds(0);
        d.setSeconds(d.getSeconds() + 1);
    } else {
        d.setSeconds(0, 0);
        d.setMinutes(d.getMinutes() + 1);
    }

    const maxYear = from.getFullYear() + 8;
    let guard = 0;
    while (out.length < count && d.getFullYear() <= maxYear && guard < 500_000) {
        guard++;
        if (!p.month.values.includes(d.getMonth() + 1)) {
            d.setMonth(d.getMonth() + 1, 1);
            d.setHours(0, 0, 0, 0);
            continue;
        }
        if (!dayMatches(p, d)) {
            d.setDate(d.getDate() + 1);
            d.setHours(0, 0, 0, 0);
            continue;
        }
        if (!p.hour.values.includes(d.getHours())) {
            d.setHours(d.getHours() + 1, 0, 0, 0);
            continue;
        }
        if (!p.minute.values.includes(d.getMinutes())) {
            d.setMinutes(d.getMinutes() + 1, 0, 0);
            continue;
        }
        if (p.hasSeconds && !p.seconds.values.includes(d.getSeconds())) {
            d.setSeconds(d.getSeconds() + 1, 0);
            continue;
        }
        out.push(new Date(d.getTime()));
        if (p.hasSeconds) d.setSeconds(d.getSeconds() + 1, 0);
        else d.setMinutes(d.getMinutes() + 1, 0, 0);
    }
    return out;
}

// ─── Explanation (plain English) ─────────────────────────────────────────────
function pad(n: number): string {
    return String(n).padStart(2, '0');
}

function ordinal(n: number): string {
    const s = ['th', 'st', 'nd', 'rd'];
    const v = n % 100;
    return n + (s[(v - 20) % 10] || s[v] || s[0]);
}

function joinHuman(items: string[]): string {
    if (items.length === 0) return '';
    if (items.length === 1) return items[0];
    if (items.length === 2) return `${items[0]} and ${items[1]}`;
    return `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`;
}

function describeTime(p: ParsedCron): string {
    const minWild = p.minute.isWildcard;
    const hourWild = p.hour.isWildcard;
    const secActive = p.hasSeconds && !(p.seconds.values.length === 1 && p.seconds.values[0] === 0);

    if (p.hasSeconds && p.seconds.wholeStep && minWild && hourWild) return `every ${p.seconds.wholeStep} seconds`;
    if (p.hasSeconds && p.seconds.isWildcard && minWild && hourWild) return 'every second';
    if (minWild && hourWild) return secActive ? 'every minute (at set seconds)' : 'every minute';
    if (p.minute.wholeStep && hourWild) return `every ${p.minute.wholeStep} minutes`;
    if (p.hour.wholeStep && p.minute.values.length === 1) {
        const m = p.minute.values[0];
        return m === 0 ? `every ${p.hour.wholeStep} hours` : `every ${p.hour.wholeStep} hours at ${pad(m)} past`;
    }
    if (minWild && !hourWild) return `every minute during ${joinHuman(p.hour.values.map((h) => `${pad(h)}:00–${pad(h)}:59`))}`;

    // Explicit minute(s) and hour(s): list the HH:MM combinations if compact.
    if (!minWild && !hourWild) {
        const combos: string[] = [];
        for (const h of p.hour.values) for (const m of p.minute.values) combos.push(`${pad(h)}:${pad(m)}`);
        if (combos.length <= 8) return `at ${joinHuman(combos)}`;
    }
    const mins = joinHuman(p.minute.values.map(String));
    const hours = joinHuman(p.hour.values.map((h) => pad(h)));
    return `at minute ${mins} past hour ${hours}`;
}

function describeDayPart(p: ParsedCron): string {
    const domRestricted = !p.dayOfMonth.isWildcard;
    const dowRestricted = !p.dayOfWeek.isWildcard;
    if (!domRestricted && !dowRestricted) return 'every day';

    const domPart = p.dayOfMonth.wholeStep
        ? `every ${ordinal(p.dayOfMonth.wholeStep)} day of the month`
        : `on the ${joinHuman(p.dayOfMonth.values.map(ordinal))}`;
    const dowPart = `on ${joinHuman(p.dayOfWeek.values.map((d) => DOW_FULL[d]))}`;

    if (domRestricted && dowRestricted) return `${domPart}, or ${dowPart}`; // union
    if (domRestricted) return domPart;
    return dowPart;
}

function describeMonth(p: ParsedCron): string {
    if (p.month.isWildcard) return '';
    return `in ${joinHuman(p.month.values.map((m) => MONTH_FULL[m - 1]))}`;
}

/** A correct, readable English description of when the schedule fires. */
export function explainCron(p: ParsedCron): string {
    if (p.reboot) return 'At system startup (@reboot).';
    const parts = [describeTime(p), describeDayPart(p), describeMonth(p)].filter(Boolean);
    const s = parts.join(', ');
    return s.charAt(0).toUpperCase() + s.slice(1) + '.';
}

/** Convenience: parse + explain in one call (throws CronError on bad input). */
export function explainExpression(expr: string): string {
    return explainCron(parseCron(expr));
}

// ─── Windows Task Scheduler translation ──────────────────────────────────────
/*
  Windows Task Scheduler is NOT cron — its triggers are Daily / Weekly /
  repetition-interval / AtStartup, not five-field expressions. We translate the
  common, unambiguous shapes and are HONEST about the rest (returning ok:false
  with a reason) rather than silently approximating. Monthly + day-of-month
  triggers are a deliberate fast-follow; see reasons below.
*/
export type CronTrigger =
    | { kind: 'startup' }
    | { kind: 'daily'; hour: number; minute: number }
    | { kind: 'weekly'; daysOfWeek: number[]; hour: number; minute: number }
    | { kind: 'minutely'; everyMinutes: number }
    | { kind: 'hourly'; everyHours: number; minute: number };

export interface SchedulePlan {
    ok: boolean;
    triggers: CronTrigger[];
    summary: string[]; // one human line per trigger
    warnings: string[];
    reason?: string; // present when ok === false
}

const TRIGGER_CAP = 50;

/** Translate a parsed cron expression into Windows Task Scheduler triggers, or
 *  explain why it can't be scheduled natively. Pure + unit-tested. */
export function planSchedule(p: ParsedCron): SchedulePlan {
    const fail = (reason: string): SchedulePlan => ({ ok: false, triggers: [], summary: [], warnings: [], reason });

    if (p.reboot) return { ok: true, triggers: [{ kind: 'startup' }], summary: ['At system startup'], warnings: [] };

    const secActive = p.hasSeconds && !(p.seconds.values.length === 1 && p.seconds.values[0] === 0);
    if (secActive) return fail("Windows Task Scheduler can't schedule with second-level precision (one-minute minimum). Drop the seconds field to schedule on this PC.");

    const domR = !p.dayOfMonth.isWildcard;
    const dowR = !p.dayOfWeek.isWildcard;
    const monthR = !p.month.isWildcard;

    if (domR && dowR) return fail("This uses both day-of-month and day-of-week. Cron fires when either matches, which Windows can't express as one schedule without double-firing - split it into two separate schedules.");
    if (monthR) return fail("Specific-month schedules aren't supported on this PC yet (monthly triggers are a fast-follow). It's still explained and previewed above.");
    if (domR) return fail("Day-of-month schedules aren't supported on this PC yet (monthly triggers are a fast-follow). It's still explained and previewed above.");

    // From here: month and day-of-month are wild; day-of-week may be restricted.
    const minWild = p.minute.isWildcard;
    const hourWild = p.hour.isWildcard;

    // "Every N minutes" — minute is '*' or '*/n', hour wild.
    if ((minWild || p.minute.wholeStep !== undefined) && hourWild) {
        if (dowR) return fail("\"Every N minutes\" combined with specific weekdays isn't supported yet.");
        const n = p.minute.wholeStep ?? 1;
        if (60 % n !== 0) return fail(`"Every ${n} minutes" doesn't divide evenly into an hour, so it can't map to a repeating Windows trigger without drift.`);
        return { ok: true, triggers: [{ kind: 'minutely', everyMinutes: n }], summary: [`Every ${n} minute${n === 1 ? '' : 's'}`], warnings: [] };
    }

    // "Every N hours at minute m" — hour is '*' or '*/n', minute a single value.
    if ((hourWild || p.hour.wholeStep !== undefined) && !minWild && p.minute.wholeStep === undefined && p.minute.values.length === 1) {
        if (dowR) return fail("\"Every N hours\" combined with specific weekdays isn't supported yet.");
        const n = p.hour.wholeStep ?? 1;
        if (24 % n !== 0) return fail(`"Every ${n} hours" doesn't divide evenly into a day, so it can't map to a repeating Windows trigger without drift.`);
        const m = p.minute.values[0];
        return { ok: true, triggers: [{ kind: 'hourly', everyHours: n, minute: m }], summary: [`Every ${n} hour${n === 1 ? '' : 's'} at ${pad(m)} past`], warnings: [] };
    }

    // Fixed daily / weekly times — minute and hour are explicit finite lists.
    const minuteExplicit = !minWild && p.minute.wholeStep === undefined;
    const hourExplicit = !hourWild && p.hour.wholeStep === undefined;
    if (minuteExplicit && hourExplicit) {
        const combos: { h: number; m: number }[] = [];
        for (const h of p.hour.values) for (const m of p.minute.values) combos.push({ h, m });
        if (combos.length > TRIGGER_CAP) return fail(`That would need ${combos.length} triggers - too many. Simplify the times.`);
        const triggers: CronTrigger[] = [];
        const summary: string[] = [];
        if (dowR) {
            const days = p.dayOfWeek.values;
            for (const c of combos) {
                triggers.push({ kind: 'weekly', daysOfWeek: days, hour: c.h, minute: c.m });
                summary.push(`Weekly on ${days.map((d) => DOW_FULL[d]).join(', ')} at ${pad(c.h)}:${pad(c.m)}`);
            }
        } else {
            for (const c of combos) {
                triggers.push({ kind: 'daily', hour: c.h, minute: c.m });
                summary.push(`Daily at ${pad(c.h)}:${pad(c.m)}`);
            }
        }
        return { ok: true, triggers, summary, warnings: [] };
    }

    return fail("This pattern can't be mapped to a native Windows schedule. Try a simpler shape: every N minutes/hours, or fixed daily/weekly times.");
}
