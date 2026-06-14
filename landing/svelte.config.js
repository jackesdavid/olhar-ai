import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    // Fully prerendered (single page) → index.html carries the real content
    // (SEO + instant paint). nginx handles any unknown path back to it.
    adapter: adapter(),
  },
};

export default config;
