/**
 * Baked annotation appearances over the real engine: the annotation
 * plugin's `renderAppearances` answers the engine's
 * `page.annotations.renderAppearances` images as they came, `$host`
 * handles with a `$hostBytes` entry each. The render plugin keeps the
 * pictures it fetched (its last few sets per page, for undo), and answers a
 * repeated ask from them, so their entries stay in the host table while it
 * does, and go once it lets go of them (`releaseImage`), here when the
 * document closes. The decoded image keeps its own bytes.
 *
 * `ebook.pdf`'s page object 181 has five baked appearances. Images are BMP
 * for the reason `stamp-preview.test.ts` gives.
 */
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeAll, describe, expect, test } from 'vitest';
import { BrowserImageEncoder, createLocalEngine, type LocalImageEncoder } from '@embedpdf/engine';
import type { Kernel } from '@embedpdf/core';
import type { PageImageHandle } from '@embedpdf/engine-core';
import { AnnotationToken as AnnotationHostToken } from '@embedpdf/plugin-annotation/contract/host';

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
} from '../src';

const here = dirname(fileURLToPath(import.meta.url));
const ebook = resolve(here, '..', '..', 'viewer', 'main', 'public', 'ebook.pdf');
const page = { kind: 'objectNumber', objectNumber: 181 } as const;

let kernel: Kernel | undefined;

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

async function annotationKernel(): Promise<Kernel> {
  const engine = createLocalEngine({ runtime: { prefer: 'wasm' }, imageEncoder: bmpEncoder() });
  const k = createKernel({
    engine,
    plugins: [stagePlugin(), interactionPlugin(), annotationPlugin(), renderPlugin()],
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

type Appearance = { image: PageImageHandle };

describe('baked annotation appearances over the real engine', () => {
  test('a repeated render answers the kept pictures, and closing lets go of them', async () => {
    const k = await annotationKernel();
    const anno = k.capability(AnnotationHostToken);
    await macrotask();
    const before = kernelCounts(k).allHosts;

    const renders: Appearance[][] = [];
    renders.push((await anno.renderAppearances(page, 1)) as Appearance[]);
    await macrotask();
    // Five images, and the bytes of each.
    const held = kernelCounts(k).allHosts;
    expect(held).toBe(before + 10);

    // The same ask: the kept set, nothing new held, nothing let go of.
    renders.push((await anno.renderAppearances(page, 1)) as Appearance[]);
    await macrotask();
    expect(kernelCounts(k).allHosts).toBe(held);

    renders.push((await anno.renderAppearances(page, 2)) as Appearance[]);
    await macrotask();
    expect(kernelCounts(k).allHosts).toBe(held + 10);

    for (const images of renders) {
      expect(images).toHaveLength(5);
      for (const { image } of images) {
        expect(image.source.kind).toBe('bytes');
        const { bytes } = image.source as { bytes: Uint8Array };
        expect(bytes.byteLength).toBeGreaterThan(0);
        const url = await image.objectUrl();
        expect(url.url).toMatch(/^blob:/);
        url.revoke();
      }
    }

    await k.documents.close('ebook');
    await macrotask();
    expect(kernelCounts(k).allHosts).toBeLessThanOrEqual(before);
  }, 60_000);
});
