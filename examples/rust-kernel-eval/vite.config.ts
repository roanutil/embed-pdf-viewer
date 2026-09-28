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

export default defineConfig({
  plugins: [rustKernel(), tailwindcss()],
  resolve: {
    alias: [
      { find: 'react-dom/client', replacement: preact('preact/compat/client') },
      { find: 'react-dom', replacement: preact('preact/compat') },
      { find: 'react/jsx-runtime', replacement: preact('preact/jsx-runtime') },
      { find: 'react/jsx-dev-runtime', replacement: preact('preact/jsx-dev-runtime') },
      { find: /^react$/, replacement: preact('preact/compat') },
    ],
  },
  define: { 'process.env.NODE_ENV': JSON.stringify('production') },
  // The dev harness's demo PDF and font.
  publicDir: path.join(root, 'packages/viewer/main/public'),
  server: { port: 5250, strictPort: true },
});
