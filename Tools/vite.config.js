import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { fileURLToPath } from "node:url";

// The pages were written against Tauri; in Search they run in a tab, and
// every Tauri module they import is answered by the shim (src/shim), which
// talks to the Search tab holding the page.
const shim = (file) => fileURLToPath(new URL(`./src/shim/${file}`, import.meta.url));

const tauri = {
  "@tauri-apps/api/core": "core.ts",
  "@tauri-apps/api/event": "event.ts",
  "@tauri-apps/api/path": "path.ts",
  "@tauri-apps/api/app": "plugins.ts",
  "@tauri-apps/api/dpi": "window.ts",
  "@tauri-apps/api/window": "window.ts",
  "@tauri-apps/api/webview": "window.ts",
  "@tauri-apps/api/webviewWindow": "window.ts",
  "@tauri-apps/plugin-dialog": "plugins.ts",
  "@tauri-apps/plugin-fs": "plugins.ts",
  "@tauri-apps/plugin-opener": "plugins.ts",
  "@tauri-apps/plugin-notification": "plugins.ts",
  "@tauri-apps/plugin-autostart": "plugins.ts",
};

export default defineConfig({
  plugins: [sveltekit()],
  resolve: {
    // Tests that mount a page (in jsdom) need Svelte's browser build.
    ...(process.env.VITEST ? { conditions: ["browser"] } : {}),
    alias: Object.entries(tauri).map(([module, file]) => ({
      find: new RegExp(`^${module.replace(/[/-]/g, "\\$&")}$`),
      replacement: shim(file),
    })),
  },
  optimizeDeps: {
    include: ["@lucide/svelte", "marked"],
  },
  build: {
    // Loaded from disk by the browser, not over a network: size matters
    // less than it would on the web, but keep the warning meaningful.
    chunkSizeWarningLimit: 1500,
  },
  server: {
    port: 1420,
    strictPort: true,
  },
});
