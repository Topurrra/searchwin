import { describe, it, expect } from 'vitest';
import { evaluateExpression, evaluateDocument } from './calcEngine';
import { resolveUnit, convert } from './calcUnits';

const ev = (s: string) => evaluateExpression(s);

describe('arithmetic & functions', () => {
    it('respects precedence and parens', () => {
        expect(ev('2 + 3 * 4')).toBe('14');
        expect(ev('(2 + 3) * 4')).toBe('20');
        expect(ev('2 ^ 10')).toBe('1,024');
        expect(ev('5!')).toBe('120');
        expect(ev('-3 + 10')).toBe('7');
    });
    it('scientific functions', () => {
        expect(ev('sqrt(9)')).toBe('3');
        expect(ev('pow(2, 8)')).toBe('256');
        expect(ev('root(27, 3)')).toBe('3');
        expect(ev('sin(30)')).toBe('0.5'); // degrees by default
        expect(ev('log(1000)')).toBe('3');
    });
    it('constants', () => {
        expect(ev('pi').startsWith('3.14159')).toBe(true);
    });
});

describe('percentages (natural language)', () => {
    it('percent of / off / additive', () => {
        expect(ev('20% of 80')).toBe('16');
        expect(ev('200 - 15%')).toBe('170');
        expect(ev('100 + 10%')).toBe('110');
        expect(ev('15% off 200')).toBe('170');
        expect(ev('20%')).toBe('20%');
    });
});

describe('units', () => {
    it('converts with in/to/as', () => {
        expect(ev('10 km in miles')).toContain('mi');
        expect(ev('10 km in miles')).toContain('6.21');
        expect(ev('2 kg to lb')).toContain('lb');
        expect(ev('2 kg to lb')).toContain('4.4');
        expect(ev('1 GB as MB')).toBe('1,000 MB');
    });
    it('temperature (offset) conversion', () => {
        expect(ev('100 c in f')).toBe('212 °F');
        expect(ev('0 c in f')).toBe('32 °F');
    });
    it('unit-aware arithmetic', () => {
        expect(ev('2 * 5 km')).toBe('10 km');
        expect(ev('10 km / 2')).toBe('5 km');
        expect(ev('10 km / 5 km')).toBe('2'); // ratio → plain number
        expect(ev('5 km + 3')).toBe('8 km'); // bare number adopts the unit
        expect(ev('5 ft + 3 in')).toBe('5.25 ft');
        expect(ev('100 km - 10%')).toBe('90 km'); // percent on a quantity
    });
    it('rejects incompatible units', () => {
        expect(() => ev('2 kg + 3 m')).toThrow();
    });
});

describe('variables & line references (multi-line document)', () => {
    it('variables carry across lines', () => {
        const r = evaluateDocument('price = 20\ntax = price * 0.1\nprice + tax');
        expect(r[0].kind).toBe('assignment');
        expect(r[0].display).toBe('20');
        expect(r[1].display).toBe('2');
        expect(r[2].display).toBe('22');
    });
    it('prev references the previous result', () => {
        const r = evaluateDocument('10\nprev + 5');
        expect(r[1].display).toBe('15');
    });
    it('sum / total runs the column', () => {
        const r = evaluateDocument('10\n20\n12\ntotal');
        expect(r[3].display).toBe('42');
    });
    it('blank lines and comments produce no result', () => {
        const r = evaluateDocument('# shopping\n5\n\n3');
        expect(r[0].kind).toBe('comment');
        expect(r[0].display).toBe('');
        expect(r[2].kind).toBe('empty');
        expect(r[1].display).toBe('5');
        expect(r[3].display).toBe('3');
    });
    it('an error on one line does not break the others', () => {
        const r = evaluateDocument('10\n2 kg + 3 m\n7');
        expect(r[0].display).toBe('10');
        expect(r[1].kind).toBe('error');
        expect(r[2].display).toBe('7');
    });
});

describe('date math', () => {
    it('adds days to an ISO date', () => {
        expect(ev('2026-01-01 + 90 days')).toContain('2026-04-01');
        expect(ev('2026-01-01 + 30 days')).toContain('2026-01-31');
    });
    it('subtracts a span from a date', () => {
        expect(ev('2026-04-01 - 1 day')).toContain('2026-03-31');
    });
    it('adds sub-day spans (hours) to a date', () => {
        expect(ev('2026-01-01 + 48 hours')).toContain('2026-01-03');
    });
    it('date minus date yields whole days', () => {
        expect(ev('2026-03-01 - 2026-01-01')).toBe('59 days');
    });
    it('formats a date with its weekday', () => {
        // 2026-01-01 is a Thursday
        expect(ev('2026-01-01')).toBe('2026-01-01 (Thursday)');
    });
    it('today/tomorrow/yesterday resolve to dates', () => {
        expect(ev('today')).toMatch(/^\d{4}-\d{2}-\d{2} \(\w+\)$/);
        expect(ev('tomorrow')).toMatch(/^\d{4}-\d{2}-\d{2} \(\w+\)$/);
        expect(ev('yesterday')).toMatch(/^\d{4}-\d{2}-\d{2} \(\w+\)$/);
    });
    it('dates work with named variables across lines', () => {
        const r = evaluateDocument('start = 2026-01-01\nstart + 30 days');
        expect(r[1].display).toContain('2026-01-31');
    });
    it('sum/total skips dates in the column', () => {
        const r = evaluateDocument('2026-01-01\n10\n20\ntotal');
        expect(r[3].display).toBe('30');
    });
    it('rejects nonsense date operations', () => {
        expect(() => ev('2026-01-01 * 2')).toThrow();
    });
});

describe('calcUnits direct', () => {
    it('resolves aliases & plurals', () => {
        expect(resolveUnit('kilometers')?.kind).toBe('linear');
        expect(resolveUnit('miles')?.kind).toBe('linear');
        expect(resolveUnit('celsius')).toEqual({ kind: 'temp', t: 'c' });
        expect(resolveUnit('banana')).toBe(null);
    });
    it('converts length', () => {
        const km = resolveUnit('km')!;
        const mi = resolveUnit('mi')!;
        expect(Math.round(convert(1, km, mi) * 1000) / 1000).toBe(0.621);
    });
});
