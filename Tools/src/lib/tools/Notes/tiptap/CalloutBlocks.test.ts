import { describe, expect, it } from 'vitest';
import { calloutKindFromText } from './CalloutBlocks';

describe('calloutKindFromText', () => {
    it('recognizes portable Markdown callout markers', () => {
        expect(calloutKindFromText('[!WARNING] Check this first')).toEqual({
            kind: 'warning',
            markerLength: 11,
        });
        expect(calloutKindFromText('ordinary quote')).toBeNull();
    });
});
