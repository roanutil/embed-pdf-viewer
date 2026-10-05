import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';

const arm = JSON.parse(process.env.PERF_ARM);
const worktree = process.env.PERF_WORKTREE;
const viewerMain = path.join(worktree, 'packages/viewer/main');
// ESM resolution, as packages/viewer/main/vite.config.ts does it. This file
// runs from the staged copy inside that package, so `import.meta.resolve`
// finds the same preact. `require.resolve` would pick the CommonJS builds.
const preact = (s) => fileURLToPath(import.meta.resolve(s));
// `preact/compat` resolved for Node gives compat/dist/compat.mjs, while
// compat/client.mjs imports `preact/compat` by name and Vite resolves that
// for the browser, compat/dist/compat.module.js: two copies of compat in one
// bundle. An arm with `dedupeCompat` aliases react and react-dom to the
// browser build as well, so there is one.
const compat = arm.dedupeCompat
  ? fileURLToPath(new URL('./compat.module.js', import.meta.resolve('preact/compat')))
  : preact('preact/compat');

// Copied from examples/rust-kernel-eval/vite.config.ts on the plugins branch.
// Kept here, not imported, so the other two worktrees build without it.
function rustKernel() {
  const coreWasm = path.join(worktree, 'packages/core-wasm');
  const redirects = path.join(coreWasm, 'src/redirect');
  const REDIRECTED = /^@embedpdf\/(core|plugin-[a-z0-9-]+)$/;
  return {
    name: 'rust-kernel',
    enforce: 'pre',
    resolveId(id, importer) {
      const match = REDIRECTED.exec(id);
      if (!match || !importer) return null;
      const importerPath = importer.replace(/^\0/, '').replace(/[?#].*$/, '');
      if (path.resolve(importerPath).startsWith(coreWasm + path.sep)) return null;
      const target = path.join(redirects, `${match[1]}.ts`);
      if (!existsSync(target)) throw new Error(`[rust-kernel] no redirect for ${id}`);
      return target;
    },
  };
}

const kernel =
  arm.kernel === 'rust'
    ? path.join(worktree, 'examples/rust-kernel-eval/perf-kernel-rust.ts')
    : path.join(process.env.PERF_ENTRY, 'kernel-typescript.ts');

export default defineConfig({
  root: process.env.PERF_ENTRY,
  publicDir: process.env.PERF_FIXTURES,
  plugins: [...(arm.kernel === 'rust' ? [rustKernel()] : []), tailwindcss()],
  resolve: {
    alias: [
      { find: '@perf/door', replacement: path.join(viewerMain, 'src/doors/local.ts') },
      { find: '@perf/kernel', replacement: kernel },
      { find: 'react-dom/client', replacement: preact('preact/compat/client') },
      { find: 'react-dom', replacement: compat },
      { find: 'react/jsx-runtime', replacement: preact('preact/jsx-runtime') },
      { find: 'react/jsx-dev-runtime', replacement: preact('preact/jsx-dev-runtime') },
      { find: /^react$/, replacement: compat },
    ],
  },
  define: {
    __PERF_ARM__: JSON.stringify(arm),
    'process.env.NODE_ENV': JSON.stringify('production'),
  },
  build: {
    outDir: process.env.PERF_OUT,
    emptyOutDir: true,
    target: 'es2022',
    sourcemap: false,
  },
});
