import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, relative, sep } from 'node:path';
import { defineConfig } from 'vite';
import { svelte, vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** Writes every emitted file and a build-specific version into the built sw.js, so it can precache the whole app. */
function precache() {
  let outDir = 'dist';
  const files = (dir) =>
    readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? files(join(dir, e.name)) : [join(dir, e.name)]));
  return {
    name: 'precache-sw',
    apply: 'build',
    configResolved(config) {
      outDir = config.build.outDir;
    },
    closeBundle() {
      const sw = join(outDir, 'sw.js');
      const assets = files(outDir)
        .map((f) => relative(outDir, f).split(sep).join('/'))
        .filter((f) => f !== 'sw.js')
        .sort();
      // Names and contents: files without a hash in their name (icon, manifest) count too.
      const hash = createHash('sha1');
      for (const f of assets) hash.update(f).update(readFileSync(join(outDir, f)));
      const version = hash.digest('hex').slice(0, 10);
      const src = readFileSync(sw, 'utf8')
        .replace("const VERSION = 'dev';", `const VERSION = ${JSON.stringify(version)};`)
        .replace('const ASSETS = [];', `const ASSETS = ${JSON.stringify(assets)};`);
      writeFileSync(sw, src);
    },
  };
}

export default defineConfig({
  // Relative base so the app works under a GitHub Pages sub-path.
  base: './',
  plugins: [svelte({ preprocess: vitePreprocess() }), precache()],
  worker: { format: 'es' },
  build: { target: 'es2022' },
});
