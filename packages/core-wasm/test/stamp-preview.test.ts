/**
 * The stamp plugin's real preview path, which until now only a manual
 * browser check covered: a library PDF imported over the real Node engine,
 * each page rendered by the engine bridge (`page.render.image`), and the
 * preview's bytes read back through the `$hostBytes` resolver the wasm
 * instance must have installed (`set_host_bytes_source`, which
 * `browser.ts`'s `initWasm()` calls in the browser and this file calls
 * itself). Without that resolver a preview has no bytes.
 *
 * Stamp previews ask for PNG, and the local engine's PNG encoder needs a
 * canvas plain Node lacks, so the engine here encodes every image as BMP
 * (the encoder's own canvas-free path). The bytes still come from a real
 * render; only their container differs.
 *
 * A preview copies its bytes out of the render, so the render's two host
 * entries (the handle and its `$hostBytes`) must go once it has: the stamp
 * plugin hands the result back through `epdf_kernel::bytes::release_host`,
 * which reaches `releaseHost` because this file installs it
 * (`set_host_release`), as `initWasm()` does in the browser.
 */
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeAll, describe, expect, test } from 'vitest';
import { BrowserImageEncoder, createLocalEngine, type LocalImageEncoder } from '@embedpdf/engine';
import type { Kernel } from '@embedpdf/core';
import { StampToken } from '@embedpdf/plugin-stamp';

import { WasmKernel, set_host_bytes_source, set_host_release } from '../wasm-node/epdf_web.js';
import { hostBytes, releaseHost } from '../src/engine-bridge';
import {
  annotationPlugin,
  createKernel,
  interactionPlugin,
  kernelCounts,
  renderPlugin,
  setWasmKernel,
  stagePlugin,
  stampPlugin,
} from '../src';

const here = dirname(fileURLToPath(import.meta.url));
const ebook = resolve(here, '..', '..', 'viewer', 'main', 'public', 'ebook.pdf');
const library = resolve(here, '..', '..', 'default-stamps', 'en', 'stamps.pdf');

let kernel: Kernel | undefined;

/** Every render as BMP, whatever format was asked for. */
function bmpEncoder(): LocalImageEncoder {
  const inner = new BrowserImageEncoder({ worker: false });
  return {
    encode: (raster, options, signal) =>
      inner.encode(raster, { ...options, format: 'bmp' }, signal),
  };
}

beforeAll(() => {
  setWasmKernel(WasmKernel);
  set_host_bytes_source(hostBytes);
  set_host_release(releaseHost);
});
afterEach(async () => {
  await kernel?.destroy();
  kernel = undefined;
});

/** A started kernel with stamp and what it depends on, `ebook.pdf` open. */
async function stampKernel(): Promise<Kernel> {
  const engine = createLocalEngine({ runtime: { prefer: 'wasm' }, imageEncoder: bmpEncoder() });
  const k = createKernel({
    engine,
    plugins: [
      stagePlugin(),
      interactionPlugin(),
      annotationPlugin(),
      renderPlugin(),
      stampPlugin(),
    ],
  });
  kernel = k;
  await k.start();
  const ready = new Promise<void>((done) => {
    const off = k.documents.onOpened(() => {
      off();
      done();
    });
  });
  const bytes = new Uint8Array(await readFile(ebook));
  k.documents.openAll([{ source: { kind: 'bytes', id: 'ebook', bytes }, scope: ['*'] }]);
  await ready;
  return k;
}

/** One macrotask: what `release` waits before it deletes. */
const macrotask = () => new Promise<void>((done) => setTimeout(done, 0));

describe('stamp asset previews over the real engine', () => {
  test('an imported library asset has a preview with its rendered bytes', async () => {
    const k = await stampKernel();
    const stamp = k.capability(StampToken);
    await stamp.importLibrary(new Uint8Array(await readFile(library)));
    const assets = stamp.listAssets();
    expect(assets.length).toBeGreaterThan(0);

    const preview = stamp.getAssetPreview(assets[0].id);
    expect(preview?.bytes).toBeInstanceOf(Uint8Array);
    expect(preview!.bytes!.byteLength).toBeGreaterThan(0);
  }, 60_000);

  test('an import leaves the engine bridge host table where it started', async () => {
    const k = await stampKernel();
    const stamp = k.capability(StampToken);
    await macrotask();
    const before = kernelCounts(k).allHosts;

    await stamp.importLibrary(new Uint8Array(await readFile(library)));
    await macrotask();

    expect(kernelCounts(k).allHosts).toBe(before);
    const assets = stamp.listAssets();
    expect(assets.length).toBeGreaterThan(0);
    for (const asset of assets) {
      expect(stamp.getAssetPreview(asset.id)!.bytes!.byteLength).toBeGreaterThan(0);
    }
  }, 60_000);
});
