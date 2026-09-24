import { describe, it, expect } from 'vitest';
import { parseCron, explainCron, explainExpression, nextRuns, validateCron, planSchedule, CronError } from './cronEngine';

const plan = (expr: string) => planSchedule(parseCron(expr));

/** Local-time 'YYYY-MM-DD HH:mm' (optionally :ss) for stable assertions. */
function fmt(d: Date, withSec = false): string {
    const p = (n: number) => String(n).padStart(2, '0');
    const base = `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
    return withSec ? `${base}:${p(d.getSeconds())}` : base;
}
const runs = (expr: string, from: Date, n: number, withSec = false) => nextRuns(parseCron(expr), from, n).map((d) => fmt(d, withSec));

describe('parsing', () => {
    it('accepts 5-field and 6-field forms', () => {
        expect(parseCron('0 9 * * *').hasSeconds).toBe(false);
        expect(parseCron('30 0 9 * * *').hasSeconds).toBe(true);
    });
    it('expands ranges, lists and steps', () => {
        expect(parseCron('0 9-17/4 * * *').hour.values).toEqual([9, 13, 17]);
        expect(parseCron('0 0 * * 1,3,5').dayOfWeek.values).toEqual([1, 3, 5]);
        expect(parseCron('*/15 * * * *').minute.values).toEqual([0, 15, 30, 45]);
    });
    it('resolves month and weekday names', () => {
        expect(parseCron('0 0 1 jan-mar *').month.values).toEqual([1, 2, 3]);
        expect(parseCron('0 9 * * mon-fri').dayOfWeek.values).toEqual([1, 2, 3, 4, 5]);
    });
    it('treats weekday 7 as Sunday', () => {
        expect(parseCron('0 0 * * 7').dayOfWeek.values).toEqual([0]);
    });
    it('expands macros', () => {
        expect(parseCron('@daily').normalized).toBe('0 0 * * *');
        expect(parseCron('@hourly').minute.values).toEqual([0]);
        expect(parseCron('@reboot').reboot).toBe(true);
    });
});

describe('validation', () => {
    it('flags wrong field counts and out-of-range values', () => {
        expect(validateCron('* * * *')).toMatch(/5 or 6 fields/);
        expect(validateCron('60 * * * *')).toMatch(/minute out of range/);
        expect(validateCron('* 24 * * *')).toMatch(/hour out of range/);
        expect(validateCron('* * * * 9')).toMatch(/day-of-week out of range/);
        expect(validateCron('*/0 * * * *')).toMatch(/Step must be greater than 0/);
        expect(validateCron('0 9 * * *')).toBeNull();
    });
    it('throws CronError (typed) on bad input', () => {
        expect(() => parseCron('nonsense here now ok go')).toThrow(CronError);
    });
});

describe('explanation', () => {
    it('describes common schedules', () => {
        expect(explainExpression('0 9 * * *')).toBe('At 09:00, every day.');
        expect(explainExpression('*/15 * * * *')).toBe('Every 15 minutes, every day.');
        expect(explainExpression('0 9 * * 1-5')).toBe('At 09:00, on Monday, Tuesday, Wednesday, Thursday and Friday.');
        expect(explainExpression('0 0 1,15 * *')).toBe('At 00:00, on the 1st and 15th.');
        expect(explainExpression('0 9,17 * * *')).toBe('At 09:00 and 17:00, every day.');
        expect(explainExpression('@reboot')).toBe('At system startup (@reboot).');
    });
    it('uses union wording when both day fields are restricted', () => {
        expect(explainExpression('0 0 13 * 5')).toBe('At 00:00, on the 13th, or on Friday.');
    });
    it('describes a months restriction and seconds steps', () => {
        expect(explainExpression('0 0 1 jan,jul *')).toBe('At 00:00, on the 1st, in January and July.');
        expect(explainExpression('*/30 * * * * *')).toBe('Every 30 seconds, every day.');
    });
});

describe('next runs', () => {
    const noon = new Date(2026, 0, 1, 12, 0, 0); // Thu 2026-01-01 12:00 local

    it('daily at midnight', () => {
        expect(runs('0 0 * * *', noon, 2)).toEqual(['2026-01-02 00:00', '2026-01-03 00:00']);
    });
    it('every 15 minutes rolls the hour', () => {
        expect(runs('*/15 * * * *', new Date(2026, 0, 1, 0, 0, 0), 4)).toEqual([
            '2026-01-01 00:15',
            '2026-01-01 00:30',
            '2026-01-01 00:45',
            '2026-01-01 01:00',
        ]);
    });
    it('weekday-only skips the weekend', () => {
        const sat = new Date(2026, 0, 3, 10, 0, 0); // 2026-01-03 is a Saturday
        expect(runs('0 9 * * 1-5', sat, 1)).toEqual(['2026-01-05 09:00']); // next Monday
    });
    it('honors the day-of-month OR day-of-week union', () => {
        // "Fridays, or the 13th" — Jan 2026: 13th is Tue, Fridays 2,9,16,23,30.
        expect(runs('0 0 13 * 5', new Date(2026, 0, 8, 0, 0, 0), 3)).toEqual([
            '2026-01-09 00:00', // Friday
            '2026-01-13 00:00', // the 13th (a Tuesday)
            '2026-01-16 00:00', // Friday
        ]);
    });
    it('projects across a month boundary', () => {
        expect(runs('0 0 1 * *', noon, 2)).toEqual(['2026-02-01 00:00', '2026-03-01 00:00']);
    });
    it('returns nothing for an impossible date', () => {
        expect(runs('0 0 30 2 *', noon, 3)).toEqual([]); // Feb 30 never happens
    });
    it('supports a 6-field seconds schedule', () => {
        expect(runs('*/30 * * * * *', new Date(2026, 0, 1, 0, 0, 0), 3, true)).toEqual([
            '2026-01-01 00:00:30',
            '2026-01-01 00:01:00',
            '2026-01-01 00:01:30',
        ]);
    });
    it('@reboot has no scheduled runs', () => {
        expect(runs('@reboot', noon, 5)).toEqual([]);
    });
});

describe('Task Scheduler translation', () => {
    it('maps every-N-minutes to a repetition trigger', () => {
        const r = plan('*/15 * * * *');
        expect(r.ok).toBe(true);
        expect(r.triggers).toEqual([{ kind: 'minutely', everyMinutes: 15 }]);
    });
    it('maps every-minute to a 1-minute repetition', () => {
        expect(plan('* * * * *').triggers).toEqual([{ kind: 'minutely', everyMinutes: 1 }]);
    });
    it('maps every-N-hours to an hourly trigger', () => {
        expect(plan('0 */2 * * *').triggers).toEqual([{ kind: 'hourly', everyHours: 2, minute: 0 }]);
    });
    it('maps a fixed daily time', () => {
        expect(plan('0 9 * * *').triggers).toEqual([{ kind: 'daily', hour: 9, minute: 0 }]);
    });
    it('expands multiple daily times into multiple triggers', () => {
        const r = plan('0 9,17 * * *');
        expect(r.triggers).toEqual([
            { kind: 'daily', hour: 9, minute: 0 },
            { kind: 'daily', hour: 17, minute: 0 },
        ]);
    });
    it('maps weekday schedules to a weekly trigger', () => {
        const r = plan('0 9 * * MON-FRI');
        expect(r.ok).toBe(true);
        expect(r.triggers).toEqual([{ kind: 'weekly', daysOfWeek: [1, 2, 3, 4, 5], hour: 9, minute: 0 }]);
    });
    it('maps @reboot to a startup trigger', () => {
        expect(plan('@reboot').triggers).toEqual([{ kind: 'startup' }]);
    });
    it('honestly refuses what Windows cannot express', () => {
        expect(plan('*/7 * * * *').ok).toBe(false); // does not divide an hour
        expect(plan('0 0 13 * 5').ok).toBe(false); // dom AND dow (union)
        expect(plan('0 0 1 * *').ok).toBe(false); // day-of-month (monthly fast-follow)
        expect(plan('0 0 1 JAN *').ok).toBe(false); // specific month
        expect(plan('*/30 * * * * *').ok).toBe(false); // sub-minute granularity
        expect(plan('*/7 * * * *').reason).toBeTruthy();
    });
});
