import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [sveltekit()],
  optimizeDeps: {
    // Lazy-loaded screens introduce some package entrypoints only on first open.
    // Pre-bundling them upfront avoids Vite's one-time full reload in dev when a
    // user opens a tool for the first time after startup.
    include: [
      "@lucide/svelte",
      "@tauri-apps/api/core",
      "@tauri-apps/api/event",
      "@tauri-apps/api/webview",
      "@tauri-apps/plugin-autostart",
      "@tauri-apps/plugin-dialog",
      "@tauri-apps/plugin-fs",
      "marked",
    ],
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
