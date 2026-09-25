// After the build: dist/catalog.json, the tools Search shows (src/lib/searchTools.ts),
// for the field to offer. Search reads it from disk beside the pages
// (SearchKit.Web.ToolCatalog). The catalog imports Svelte icons, so Vite loads it.
import { writeFileSync } from 'node:fs';
import { createServer } from 'vite';

const vite = await createServer({ server: { middlewareMode: true, hmr: false }, appType: 'custom', logLevel: 'error' });
try {
    const { catalog } = await vite.ssrLoadModule('/src/lib/searchTools.ts');
    const tools = catalog();
    writeFileSync('dist/catalog.json', JSON.stringify(tools));
    console.log(`catalog.json: ${tools.length} tools`);
} finally {
    await vite.close();
}
