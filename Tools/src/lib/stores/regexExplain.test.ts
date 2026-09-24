import { describe, it, expect } from 'vitest';
import { explainPattern } from './regexExplain';

const types = (p: string) => explainPattern(p).map((t) => t.type);
const texts = (p: string) => explainPattern(p).map((t) => t.text);
const label = (p: string, text: string) => explainPattern(p).find((t) => t.text === text)?.label ?? '';

describe('regexExplain', () => {
    it('anchors and literal runs', () => {
        expect(explainPattern('^abc$')).toEqual([
            { text: '^', type: 'anchor', label: expect.stringContaining('Start of the string') },
            { text: 'abc', type: 'literal', label: expect.stringContaining('Literal text') },
            { text: '$', type: 'anchor', label: expect.stringContaining('End of the string') },
        ]);
    });
    it('classes and counted quantifiers', () => {
        expect(texts('\\d{3}-\\d{4}')).toEqual(['\\d', '{3}', '-', '\\d', '{4}']);
        expect(types('\\d{3}-\\d{4}')).toEqual(['class', 'quantifier', 'literal', 'class', 'quantifier']);
        expect(label('\\d{3}', '{3}')).toContain('exactly 3 times');
        expect(label('\\d{2,4}', '{2,4}')).toContain('between 2 and 4 times');
        expect(label('\\d{2,}', '{2,}')).toContain('2 or more times');
    });
    it('greedy vs lazy quantifiers', () => {
        expect(label('a+', '+')).toContain('greedy');
        expect(label('a+?', '+?')).toContain('lazy');
    });
    it('group kinds', () => {
        expect(label('(?<year>\\d{4})', '(?<year>')).toContain('named capturing group "year"');
        expect(label('(?:abc)', '(?:')).toContain('non-capturing');
        expect(label('(abc)', '(')).toContain('capturing group');
    });
    it('lookaround', () => {
        expect(label('(?=\\d)', '(?=')).toContain('lookahead');
        expect(label('(?<!x)', '(?<!')).toContain('Negative lookbehind');
    });
    it('character classes with ranges and negation', () => {
        expect(label('[A-Za-z0-9_]', '[A-Za-z0-9_]')).toContain('A-Z, a-z, 0-9, _');
        expect(label('[^0-9]', '[^0-9]')).toContain('NOT in');
    });
    it('alternation', () => {
        expect(types('foo|bar')).toEqual(['literal', 'alternation', 'literal']);
    });
    it('escapes and word boundary', () => {
        expect(label('\\.', '\\.')).toContain('Literal "."');
        expect(label('\\bword\\b', '\\b')).toContain('Word boundary');
        expect(label('\\n', '\\n')).toContain('Newline');
    });
    it('backreferences (numbered and named)', () => {
        expect(label('(a)\\1', '\\1')).toContain('Backreference to group 1');
        expect(label('(?<x>a)\\k<x>', '\\k<x>')).toContain('named group "x"');
    });
    it('inline flag group', () => {
        expect(label('(?i)abc', '(?i)')).toContain('Inline flags: i');
    });
    it('the cleaned email pattern tokenizes cleanly', () => {
        expect(types('\\w+@\\w+\\.\\w+')).toEqual([
            'class', 'quantifier', 'literal', 'class', 'quantifier', 'escape', 'class', 'quantifier',
        ]);
    });
});
