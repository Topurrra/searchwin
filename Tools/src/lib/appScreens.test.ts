import { describe, expect, it } from 'vitest';

import {
    aboutQuickStarts,
    allScreens,
    builtInProfileDefinitions,
    categoryScreenIdForTool,
    documentationScreens,
    getScreen,
    screensForPacks,
    toolPackIdForScreen,
    toolPacks,
    toolIdsByCategory,
    toolIdsByList,
    toolScreens,
    toolScreensForPacks,
} from '$lib/appScreens';

describe('app screen catalog', () => {
    it('uses unique screen ids', () => {
        const ids = allScreens.map((screen) => screen.id);
        expect(new Set(ids).size).toBe(ids.length);
    });

    it('keeps tool list order when resolving explicit ids', () => {
        expect(toolIdsByList(['image-studio', 'hash-check', 'missing-id'])).toEqual(['hash-check', 'image-studio']);
    });

    it('resolves category groupings from the shared tool catalog', () => {
        const developerIds = toolIdsByCategory(['Development']);

        expect(developerIds.length).toBeGreaterThan(0);
        expect(developerIds).toContain('dev-toolkit');
        expect(developerIds).toContain('ssh-key-manager');
    });

    it('keeps core pack minimal and filters optional packs', () => {
        const coreTools = toolScreensForPacks(['core']);
        expect(coreTools.map((tool) => tool.id)).toEqual([]);
        expect(screensForPacks(['core']).some((screen) => screen.id === 'settings')).toBe(true);
        expect(toolScreensForPacks(['core', 'privacy']).some((tool) => tool.id === 'file-shredder')).toBe(true);
    });

    it('keeps media workflows together without exposing a disabled pack workspace', () => {
        expect(categoryScreenIdForTool('screen-recorder')).toBe('category-media');
        expect(categoryScreenIdForTool('media-utility')).toBe('category-media');
        expect(screensForPacks(['core']).some((screen) => screen.id === 'category-media')).toBe(false);
        expect(screensForPacks(['core', 'media']).some((screen) => screen.id === 'category-media')).toBe(true);
    });

    it('assigns every tool to a declared pack', () => {
        const declaredPackIds = new Set(toolPacks.map((pack) => pack.id));
        for (const tool of toolScreens) {
            expect(declaredPackIds.has(toolPackIdForScreen(tool))).toBe(true);
        }
    });

    it('keeps documentation and quick-start links attached to real screens', () => {
        expect(documentationScreens.length).toBeGreaterThan(0);

        for (const screen of documentationScreens) {
            expect(screen.docs).toBeDefined();
            expect(toolScreens.some((tool) => tool.id === screen.id)).toBe(true);
        }

        for (const card of aboutQuickStarts) {
            expect(getScreen(card.targetId)).toBeDefined();
        }
    });

    it('keeps built-in profile definitions aligned with available tools', () => {
        for (const profile of builtInProfileDefinitions) {
            if (profile.includeToolIds) {
                expect(toolIdsByList(profile.includeToolIds)).toHaveLength(profile.includeToolIds.length);
            }

            if (profile.includeCategories) {
                expect(toolIdsByCategory(profile.includeCategories).length).toBeGreaterThan(0);
            }
        }
    });
});
