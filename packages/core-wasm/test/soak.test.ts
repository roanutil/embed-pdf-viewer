/**
 * Memory soak test: pans through `ebook.pdf` 300 times over the real Node
 * engine and records `kernelCounts` (every table `packages/core-wasm` and
 * the harness grow across the JS/Rust boundary) at iterations 50 and 300,
 * then asserts that none of them grows between the two samples. Every
 * render gets a new width, so the render plugin's raster store fills its
 * budget and evicts, and each eviction releases the image's two host
 * entries (`RenderHost::release_image`, `bridge.release`); the host table
 * is bounded by that budget rather than held flat. `heapUsed` is logged
 * but not asserted: it cannot see wasm linear memory, which is where the
 * baseline's growth was (`wasmBytes`; the measured numbers are in
 * `docs/research/rust-plugins-web-eval.md`).
 *
 * Two deviations from the obvious reading of "pan and read the plan", both
 * found by running this and checking what actually moved:
 *
 *   - `panBy(dxViewport, dyViewport)` pans the CAMERA by a viewport delta,
 *     so a positive `dy` moves the camera up the page; at the top of the
 *     document that is a no-op forever (`scrollTop` stays clamped at 0).
 *     `panBy(0, -40)` is what actually scrolls down.
 *   - `getPlan` (`ViewDemand::get_plan`, `crates/plugins/render/src/controller
 *     .rs`) is a pure read of whatever `setDemand` last planned; it never
 *     calls the engine. At this viewport the tile plane never engages
 *     either (the full-page raster already supplies enough sharpness), so a
 *     `createViewDemand` + `getPlan` loop alone never grows `hosts`. The
 *     loop below still calls `getPlan` per visible page (a page host reads
 *     it every frame) and separately drives real engine renders with
 *     `render.renderPage`, which is what actually asks the engine for
 *     pixels and grows the engine bridge's host table and the harness's
 *     promise table. `format: 'bmp'` sidesteps the local engine's canvas
 *     image encoder, unavailable under plain Node; see `THUMBNAIL_WIDTH`
 *     below for why the render is thumbnail-sized, not full width.
 */
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeAll, describe, expect, test } from 'vitest';
import { createLocalEngine } from '@embedpdf/engine';
import type { Kernel } from '@embedpdf/core';
import { StageToken } from '@embedpdf/plugin-stage';
import { RenderToken } from '@embedpdf/plugin-render/contract/host';
import type { PageImage } from '@embedpdf/plugin-render';

import { WasmKernel, wasm_memory_bytes } from '../wasm-node/epdf_web.js';
import {
  createKernel,
  interactionPlugin,
  kernelCounts,
  renderPlugin,
  setWasmKernel,
  setWasmMemoryBytes,
  stagePlugin,
} from '../src';

const here = dirname(fileURLToPath(import.meta.url));
const ebook = resolve(here, '..', '..', 'viewer', 'main', 'public', 'ebook.pdf');
const ITERATIONS = 300;
const SAMPLE_AT = [50, 300];
/** `renderPage`'s width at iteration `i` is `BASE_WIDTH + i`: a new raster
 *  key on every call, so nothing is a cache hit and the ~900 uncompressed
 *  BMP renders (300 iterations x up to ~3 visible pages, each 200px wide
 *  or more, about 200 KB and up) overflow the raster store's 128 MiB
 *  budget (`DEFAULT_MAX_COST` in `raster_store.rs`) several times over.
 *  The encoded bytes stay on the JS side (the bridge's `$hostBytes`), so
 *  the harness's promise table holds only small JSON. */
const BASE_WIDTH = 200;
/** `RasterStore`'s default budget, in the ~bytes `cost()` counts. */
const RASTER_BUDGET = 128 * 1024 * 1024;
/** Every counter `kernelCounts` reports, so a sample's shape is checked
 *  without hard-coding which tables exist. */
const COUNTER_KEYS = [
  'refs',
  'promises',
  'heldFns',
  'described',
  'aborts',
  'hosts',
  'allHosts',
  'documents',
  'fns',
  'wireRefs',
  'proxies',
  'wasmBytes',
] as const;
/** Counters whose entries are released on a deterministic path (a
 *  settled promise, a closed description, a dropped proxy), so the sample
 *  at 300 must not exceed the one at 50 at all. */
const EXACT_KEYS = ['promises', 'aborts', 'described', 'documents', 'proxies'] as const;
/** Counters with a release that can lag the sample: `refs`/`wireRefs` for
 *  a decoded loose ref wait on its `FinalizationRegistry` callback, which
 *  runs only after a GC (this file runs without `--expose-gc`, so it cannot
 *  force one), and `fns`/`heldFns` wait on the release sweep, which runs at
 *  the end of the kernel's next outermost entry (`crates/epdf-web/src/
 *  exports.rs`). They may
 *  grow by `LAGGING_SLACK` at most. The baseline grew harness `refs` from
 *  1271 to 5915 over the same 250 iterations, about 18 per iteration, so
 *  it crossed this slack within four iterations. */
const LAGGING_KEYS = ['refs', 'wireRefs', 'fns', 'heldFns'] as const;
const LAGGING_SLACK = 64;
/** Wasm linear memory only ever grows, in 64 KiB pages, so `wasmBytes` is a
 *  high-water mark: any leak, and any one-off spike, shows up as a step
 *  that never comes back. The fixed build reads the same 12,582,912 bytes
 *  at both samples. The baseline grew 16,252,928 -> 66,846,720 bytes at a
 *  96px render width, under half the smallest width rendered here: about
 *  48 MiB of growth against this 2 MiB allowance (32 pages, room for
 *  allocator fragmentation to shift with GC timing). */
const WASM_GROWTH_BOUND = 2 * 1024 * 1024;

let bytes: Uint8Array;
let kernel: Kernel | undefined;

beforeAll(async () => {
  setWasmKernel(WasmKernel);
  setWasmMemoryBytes(wasm_memory_bytes);
  bytes = new Uint8Array(await readFile(ebook));
});
afterEach(async () => {
  await kernel?.destroy();
  kernel = undefined;
});

interface Sample {
  iteration: number;
  counts: Record<string, number>;
  heapUsed: number;
}

describe('memory soak: 300 iterations of panning and rendering', () => {
  test('kernelCounts stay bounded between iterations 50 and 300', async () => {
    const engine = createLocalEngine({ runtime: { prefer: 'wasm' } });
    const k = createKernel({
      engine,
      plugins: [stagePlugin(), interactionPlugin(), renderPlugin()],
    });
    kernel = k;
    await k.start();
    const ready = new Promise<void>((done) => {
      const off = k.documents.onOpened(() => {
        off();
        done();
      });
    });
    k.documents.openAll([{ source: { kind: 'bytes', id: 'fixture', bytes }, scope: ['*'] }]);
    await ready;

    const stage = k.capability(StageToken);
    stage.setViewportSize({ width: 800, height: 600 });
    const render = k.capability(RenderToken);
    const view = render.createViewDemand('soak');

    const samples: Sample[] = [];
    // `renderPage` rejects `operation-cancelled` on abort (`RenderCapability
    // .renderPage`'s own doc comment); nothing here aborts a render, so any
    // such rejection is expected shutdown noise, not a failure. Anything
    // else is a real failure this test should not swallow.
    const renderFailures: unknown[] = [];
    const isAbort = (error: unknown): boolean =>
      (error as { code?: string } | null)?.code === 'operation-cancelled';
    let renders = 0;
    let smallest = Infinity;
    let mostVisible = 0;

    for (let i = 1; i <= ITERATIONS; i++) {
      stage.panBy(0, -40);
      const visible = stage.listVisiblePages();
      mostVisible = Math.max(mostVisible, visible.length);
      for (const page of visible) {
        view.getPlan(page.ref);
        renders++;
        void render.renderPage(page.ref, { width: BASE_WIDTH + i, format: 'bmp' }).then(
          (image: PageImage) => {
            const source = image.source as { kind: string; bytes?: Uint8Array };
            if (source.bytes) smallest = Math.min(smallest, source.bytes.byteLength);
          },
          (error: unknown) => {
            if (!isAbort(error)) renderFailures.push(error);
          },
        );
      }
      // Drain the microtask queue so this iteration's renders (and the
      // subscribe listener their store changes fire) settle before the
      // next `panBy`, as the brief specifies.
      await new Promise((r) => setTimeout(r, 0));

      if (SAMPLE_AT.includes(i)) {
        samples.push({
          iteration: i,
          counts: kernelCounts(k),
          heapUsed: process.memoryUsage().heapUsed,
        });
      }
    }

    console.log('[soak] samples:', JSON.stringify(samples, null, 2));

    expect(renderFailures, `render failures: ${JSON.stringify(renderFailures)}`).toHaveLength(0);

    // Two host entries per image (the handle and its bytes). The store keeps
    // at most its budget's worth of images, counted at the smallest image
    // rendered, plus the ones still on screen; the bound must be one that
    // an unreleased table would break, or it proves nothing.
    const budgetImages = Math.floor(RASTER_BUDGET / smallest);
    const bound = 2 * (budgetImages + mostVisible);
    const final = samples[samples.length - 1].counts;
    console.log('[soak] host bound:', { renders, smallest, budgetImages, mostVisible, bound });
    expect(2 * renders, 'the soak renders enough to overflow the bound').toBeGreaterThan(bound);
    expect(final.allHosts).toBeLessThanOrEqual(bound);
    expect(final.hosts).toBeLessThanOrEqual(bound);

    expect(samples).toHaveLength(SAMPLE_AT.length);
    for (const sample of samples) {
      for (const key of COUNTER_KEYS) {
        expect(sample.counts, `counter "${key}" at iteration ${sample.iteration}`).toHaveProperty(
          key,
        );
        expect(typeof sample.counts[key]).toBe('number');
      }
    }

    const [first, last] = samples.map((sample) => sample.counts);
    for (const key of EXACT_KEYS) {
      expect(last[key], `counter "${key}" grew from ${first[key]}`).toBeLessThanOrEqual(first[key]);
    }
    for (const key of LAGGING_KEYS) {
      expect(last[key], `counter "${key}" grew from ${first[key]}`).toBeLessThanOrEqual(
        first[key] + LAGGING_SLACK,
      );
    }
    expect(last.wasmBytes - first.wasmBytes, 'wasmBytes growth').toBeLessThanOrEqual(
      WASM_GROWTH_BOUND,
    );
  }, 60_000);
});
