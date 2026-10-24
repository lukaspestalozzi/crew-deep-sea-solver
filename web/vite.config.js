import { defineConfig } from 'vite';
import { svelte, vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  // Relative base so the app works under a GitHub Pages sub-path.
  base: './',
  plugins: [svelte({ preprocess: vitePreprocess() })],
  worker: { format: 'es' },
  build: { target: 'es2022' },
});
