import { afterEach, describe, expect, it } from 'vitest';

import {
    stageToolLaunchTarget,
    subscribeToToolLaunchTarget,
    toolLaunchTarget,
} from './toolLaunchTarget';

afterEach(() => toolLaunchTarget.set(null));

describe('tool launch targets', () => {
    it('delivers a matching target once and clears it', () => {
        const received: string[] = [];
        const stop = subscribeToToolLaunchTarget('image-studio', (target) => {
            received.push(target.targetFile);
        });

        stageToolLaunchTarget({ toolId: 'image-studio', targetFile: ' C:\\photo.png ' });
        stop();

        expect(received).toEqual(['C:\\photo.png']);
        let current: unknown;
        const inspect = toolLaunchTarget.subscribe((target) => (current = target));
        inspect();
        expect(current).toBeNull();
    });

    it('keeps a target until its matching tool subscribes', () => {
        const received: string[] = [];
        const stop = subscribeToToolLaunchTarget('ocr-image-to-text', (target) => {
            received.push(target.targetFile);
        });

        stageToolLaunchTarget({ toolId: 'image-studio', targetFile: 'C:\\photo.png' });
        stop();

        expect(received).toEqual([]);
        const matchingStop = subscribeToToolLaunchTarget('image-studio', (target) => {
            received.push(target.targetFile);
        });
        matchingStop();
        expect(received).toEqual(['C:\\photo.png']);
    });
});
