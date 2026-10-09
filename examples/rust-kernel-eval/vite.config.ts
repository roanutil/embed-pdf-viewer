/**
 * The viewer chrome from source, as `packages/viewer/main` runs it in dev,
 * with one difference: the kernel and the plugins are the Rust ones.
 *
 * The React-to-Preact aliases are viewer/main's (`packages/viewer/main/vite.config.ts`).
 * The `rust-kernel` plugin resolves the bare specifiers `@embedpdf/core` and
 * `@embedpdf/plugin-<id>` to `packages/core-wasm/src/redirect/<name>.ts`.
 * Subpaths (`@embedpdf/plugin-stage/contract/host`) resolve normally, and so
 * does every import made from inside `packages/core-wasm`: the redirect
 * modules `export *` from the real packages.
 */
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig, type Plugin } from 'vite';
import tailwindcss from '@tailwindcss/vite';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');
const coreWasm = path.join(root, 'packages/core-wasm');
const redirects = path.join(coreWasm, 'src/redirect');

// Absolute file paths, resolved from this package (see viewer/main's config).
const preact = (specifier: string) => fileURLToPath(import.meta.resolve(specifier));

const REDIRECTED = /^@embedpdf\/(core|plugin-[a-z0-9-]+)$/;

function rustKernel(): Plugin {
  return {
    name: 'rust-kernel',
    enforce: 'pre',
    resolveId(id, importer) {
      const match = REDIRECTED.exec(id);
      if (!match || !importer) return null;
      // Vite hands some importers back with a query suffix (`foo.ts?worker`)
      // or a `\0` virtual-module prefix; strip both before treating the rest
      // as a file path.
      const importerPath = importer.replace(/^\0/, '').replace(/[?#].*$/, '');
      if (path.resolve(importerPath).startsWith(coreWasm + path.sep)) return null;
      const target = path.join(redirects, `${match[1]}.ts`);
      if (!existsSync(target)) {
        throw new Error(`[rust-kernel] no redirect for ${id} (expected ${target})`);
      }
      return target;
    },
  };
}

// What the top bar names as this build's source. `dirty` means the working
// tree had uncommitted changes, so the commit alone doesn't reproduce it.
function buildInfo() {
  const git = (...args: string[]) => execFileSync('git', args, { cwd: here, encoding: 'utf8' }).trim();
  return {
    commit: git('rev-parse', 'HEAD'),
    branch: git('rev-parse', '--abbrev-ref', 'HEAD'),
    dirty: git('status', '--porcelain', '--untracked-files=no') !== '',
    builtAt: new Date().toISOString(),
  };
}

export default defineConfig({
  plugins: [rustKernel(), tailwindcss()],
  resolve: {
    // `vite build` matches `production`, where the workspace packages' exports
    // point at dist/, which nothing here builds. `development` points at src/,
    // so the build compiles the same source `pnpm eval` serves.
    conditions: ['module', 'browser', 'development'],
    alias: [
      { find: 'react-dom/client', replacement: preact('preact/compat/client') },
      { find: 'react-dom', replacement: preact('preact/compat') },
      { find: 'react/jsx-runtime', replacement: preact('preact/jsx-runtime') },
      { find: 'react/jsx-dev-runtime', replacement: preact('preact/jsx-dev-runtime') },
      { find: /^react$/, replacement: preact('preact/compat') },
    ],
  },
  define: {
    'process.env.NODE_ENV': JSON.stringify('production'),
    __BUILD_INFO__: JSON.stringify(buildInfo()),
  },
  // Both found late by the dependency scan, so a cold cache reloaded the
  // page for each: the JSX dev runtime on first load, and QuickJS, which the
  // form sandbox reaches through a `#quickjs-variant` subpath import, on the
  // first form script.
  optimizeDeps: {
    include: [
      'react/jsx-dev-runtime',
      '@embedpdf/core-wasm > @embedpdf/core-js-sandbox > @jitl/quickjs-singlefile-browser-release-sync',
    ],
  },
  // The dev harness's demo PDF and font.
  publicDir: path.join(root, 'packages/viewer/main/public'),
  server: { port: 5250, strictPort: true },
  preview: { port: 5251, strictPort: true },
  // The default target (es2020) rejects the top-level await in the bundle.
  build: { target: 'es2022' },
});
