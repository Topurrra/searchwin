// Search serves the tool pages from a folder (https://tools.search/), which
// has no server to answer deep links, so routes live after the hash:
// https://tools.search/#/tool/hash-check.
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({ pages: "dist", assets: "dist", fallback: undefined }),
    router: { type: "hash" },
  },
};

export default config;
