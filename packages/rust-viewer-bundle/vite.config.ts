/**
 * The viewer's snippet build (`packages/viewer/main/vite.config.ts`, `--mode
 * snippet`) on the Rust kernel: one self-contained, relocatable folder, with
 * every asset emitted beside the entry and referenced by a relative URL.
 *
 * Two differences from the snippet pass:
 *
 *   - The `rust-kernel` plugin (as in `examples/rust-kernel-eval`) resolves
 *     the bare specifiers `@embedpdf/core` and `@embedpdf/plugin-<id>` to
 *     `packages/core-wasm/src/redirect/<name>.ts`, except from inside
 *     `packages/core-wasm`, whose redirects `export *` from the real packages.
 *   - `epdf_web_bg.wasm` is emitted as a file, not inlined: wasm-bindgen's glue
 *     locates it with `new URL('epdf_web_bg.wasm', import.meta.url)`, which
 *     library mode would turn into 13 MB of base64.
 */
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig, type Plugin } from 'vite';
import tailwindcss from '@tailwindcss/vite';

// Anchored on the package dir: vite executes this config from a .vite-temp
// copy, so import.meta-relative paths break (see viewer/main's config).
const here = process.cwd();
const root = path.resolve(here, '../..');
const coreWasm = path.join(root, 'packages/core-wasm');
const redirects = path.join(coreWasm, 'src/redirect');
const viewerMain = path.join(root, 'packages/viewer/main');

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

const GLUE_WASM_URL = /new URL\((["'])epdf_web_bg\.wasm\1,\s*import\.meta\.url\)/;

/** The kernel's wasm as an emitted asset, as build/wasm-url-asset.js does for PDFium's. */
function kernelWasmAsAsset(): Plugin {
  let found = false;
  return {
    name: 'rust-kernel:wasm-as-asset',
    enforce: 'pre',
    apply: 'build',
    transform(code, id) {
      if (!id.split('?')[0].endsWith('/epdf_web.js') || !GLUE_WASM_URL.test(code)) return null;
      found = true;
      const file = path.join(path.dirname(id.split('?')[0]), 'epdf_web_bg.wasm');
      const rewritten = code.replace(GLUE_WASM_URL, 'new URL(__epdf_web_bg_wasm_url)');
      const footer = `import __epdf_web_bg_wasm_url from ${JSON.stringify(`${file}?url&no-inline`)};`;
      return { code: `${rewritten}\n${footer}\n`, map: null };
    },
    buildEnd(error) {
      if (!error && !found) {
        this.error(
          "no reference to epdf_web_bg.wasm was found in wasm-bindgen's glue, so the bundle " +
            'would ship without the kernel. Has the wasm-bindgen version changed?',
        );
      }
    },
  };
}

// Which source this build came from, exported as `buildInfo`.
function buildInfo() {
  const git = (...args: string[]) => execFileSync('git', args, { cwd: here, encoding: 'utf8' }).trim();
  return {
    commit: git('rev-parse', 'HEAD'),
    branch: git('rev-parse', '--abbrev-ref', 'HEAD'),
    dirty: git('status', '--porcelain', '--untracked-files=no', '--ignore-submodules') !== '',
    builtAt: new Date().toISOString(),
  };
}

export default defineConfig({
  plugins: [rustKernel(), kernelWasmAsAsset(), tailwindcss()],
  resolve: {
    // The workspace packages' production exports point at dist/, which
    // nothing here builds; `development` points at src/.
    conditions: ['module', 'browser', 'development'],
    alias: [
      { find: 'react-dom/client', replacement: preact('preact/compat/client') },
      { find: 'react-dom', replacement: preact('preact/compat') },
      { find: 'react/jsx-runtime', replacement: preact('preact/jsx-runtime') },
      { find: 'react/jsx-dev-runtime', replacement: preact('preact/jsx-dev-runtime') },
      { find: /^react$/, replacement: preact('preact/compat') },
      {
        find: '@embedpdf/engine-runtime-wasm32/wasm-url',
        replacement: path.join(viewerMain, 'build/wasm-url-asset.js'),
      },
    ],
  },
  define: {
    'process.env.NODE_ENV': JSON.stringify('production'),
    __BUILD_INFO__: JSON.stringify(buildInfo()),
  },
  // Relocatable: chunk and asset URLs resolve against the chunk, not the site root.
  experimental: {
    renderBuiltUrl: () => ({ relative: true }),
  },
  build: {
    // The kernel's glue and the entry both await at the top level.
    target: 'es2022',
    sourcemap: true,
    copyPublicDir: false,
    lib: {
      entry: { index: 'src/index.ts' },
      formats: ['es'],
      fileName: (_format, entryName) => `${entryName}.js`,
    },
    rollupOptions: {
      output: {
        chunkFileNames: 'chunks/[name]-[hash].js',
        assetFileNames: '[name][extname]',
      },
    },
  },
});
