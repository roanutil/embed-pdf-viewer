/**
 * Refs JavaScript has dropped are released, and a capability's own
 * (pinned) refs never are. A `FinalizationRegistry` callback runs when the
 * engine chooses, so these force it: this file's vitest project runs with
 * `--expose-gc` (`vitest.gc.config.ts`).
 */
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeAll, describe, expect, test, vi } from 'vitest';
import { createLocalEngine } from '@embedpdf/engine';
import type { Kernel } from '@embedpdf/core';
import { InteractionToken } from '@embedpdf/plugin-interaction';
import { StageToken } from '@embedpdf/plugin-stage';
import { RenderToken } from '@embedpdf/plugin-render/contract/host';

import { WasmKernel } from '../wasm-node/epdf_web.js';
import {
  createKernel,
  interactionPlugin,
  kernelCounts,
  renderPlugin,
  setWasmKernel,
  stagePlugin,
} from '../src';
import { Wire, type RustKernel, type RustKernelClass } from '../src/proxy';
import type { EngineBridge } from '../src/engine-bridge';

const here = dirname(fileURLToPath(import.meta.url));
const fixtures = resolve(here, '..', '..', 'engine', 'main', 'test', 'fixtures');

const gc = (globalThis as { gc?: () => void }).gc;

/** Collect until `done()` holds, a turn between tries so the registry's
 *  cleanup callbacks get to run; false if it never does. */
async function collectUntil(done: () => boolean): Promise<boolean> {
  for (let i = 0; i < 20; i++) {
    gc!();
    await new Promise((r) => setTimeout(r, 0));
    if (done()) return true;
  }
  return false;
}

let bytes: Uint8Array;
let kernel: Kernel | undefined;

beforeAll(async () => {
  setWasmKernel(WasmKernel);
  bytes = new Uint8Array(await readFile(resolve(fixtures, 'two_page_sigfield.pdf')));
});
afterEach(async () => {
  vi.restoreAllMocks();
  await kernel?.destroy();
  kernel = undefined;
});

async function openFixture(): Promise<Kernel> {
  const engine = createLocalEngine({ runtime: { prefer: 'wasm' } });
  const k = createKernel({ engine, plugins: [stagePlugin(), interactionPlugin(), renderPlugin()] });
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
  return k;
}

describe('releasing refs JavaScript has dropped', () => {
  test('the pool exposes gc()', () => {
    expect(typeof gc).toBe('function');
  });

  test('a dropped unsubscribe is released; a pinned capability ref survives', async () => {
    const k = await openFixture();
    const describe = vi.spyOn(WasmKernel.prototype, 'capability');
    const stage = k.capability(StageToken);
    const rust = describe.mock.contexts[0] as WasmKernel;
    const description = JSON.parse(describe.mock.results[0].value as string);
    expect(description.getCamera).toMatchObject({ pinned: true });
    const camera = stage.getCamera();

    // Let whatever the setup left behind go first: `openFixture`'s
    // `onOpened` listener holds its own `off`, which is collected (and its
    // ref released) only once the kernel has released the listener.
    await collectUntil(() => false);
    const before = kernelCounts(k).refs;
    // Dropped as soon as this statement ends: nothing keeps the function.
    stage.onPageChanged(() => {});
    expect(kernelCounts(k).refs).toBe(before + 1);

    expect(await collectUntil(() => kernelCounts(k).refs === before)).toBe(true);

    // The same release, asked for a capability's own member, does nothing:
    // the memoized description hands that id out again.
    rust.release_ref(description.getCamera.$ref);
    await collectUntil(() => false);
    expect(kernelCounts(k).refs).toBe(before);
    expect(stage.getCamera()).toEqual(camera);
  });
});

describe('releasing JavaScript functions Rust has dropped', () => {
  test('an unregistered handler gives its functions back, however many times', async () => {
    const k = await openFixture();
    const interaction = k.capability(InteractionToken);
    const start = kernelCounts(k).fns;
    for (let i = 0; i < 200; i++) {
      const off = interaction.registerHandler({
        id: `probe-${i}`,
        priority: 1,
        onDown: () => {},
        onMove: () => {},
      });
      if (i === 0) expect(kernelCounts(k).fns).toBe(start + 2);
      off();
    }
    expect(kernelCounts(k).fns).toBe(start);
  });

  test('a function passed twice lives until Rust drops both', async () => {
    const k = await openFixture();
    const interaction = k.capability(InteractionToken);
    const start = kernelCounts(k).fns;
    const onDown = () => {};
    const first = interaction.registerHandler({ id: 'a', priority: 1, onDown });
    const second = interaction.registerHandler({ id: 'b', priority: 1, onDown });
    expect(kernelCounts(k).fns).toBe(start + 1);
    first();
    expect(kernelCounts(k).fns).toBe(start + 1);
    second();
    expect(kernelCounts(k).fns).toBe(start);
  });

  test('a released id called from Rust throws loudly', () => {
    const wire = new Wire({ host: () => undefined, hold: () => 0 } as unknown as EngineBridge);
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const spec = wire.encode(() => 1) as { $fn: string };
    const id = spec.$fn.slice('js:'.length);
    expect(wire.callJs(id, '[]')).toBe(1);
    wire.releaseJs(id);
    expect(() => wire.callJs(id, '[]')).toThrow(`[core-wasm] js function ${id} was released`);
    expect(error).toHaveBeenCalled();
  });

  test('a release while a promise carries the function waits for the next task', async () => {
    const wire = new Wire({ host: () => undefined, hold: () => 0 } as unknown as EngineBridge);
    const f = () => 2;
    const spec = wire.encode(() => Promise.resolve(f)) as { $fn: string };
    const answer = (await wire.callJs(spec.$fn.slice('js:'.length), '[]')) as { $fn: string };
    const id = answer.$fn.slice('js:'.length);

    // The kernel has not read the settled value yet: its release of the
    // same function, from some other call, leaves `f` where it is.
    wire.releaseJs(id);
    expect(wire.callJs(id, '[]')).toBe(2);

    await new Promise((r) => setTimeout(r, 0));
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    wire.releaseJs(id);
    expect(() => wire.callJs(id, '[]')).toThrow(`[core-wasm] js function ${id} was released`);
    expect(error).toHaveBeenCalled();
  });

  test('a second release of one id warns', () => {
    const wire = new Wire({ host: () => undefined, hold: () => 0 } as unknown as EngineBridge);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const spec = wire.encode(() => 1) as { $fn: string };
    const id = spec.$fn.slice('js:'.length);
    wire.releaseJs(id);
    expect(warn).not.toHaveBeenCalled();
    wire.releaseJs(id);
    expect(warn).toHaveBeenCalledWith(`[core-wasm] release_js(${id}): already released`);
  });
});

describe('a closed document', () => {
  test('gives its images back once its capabilities are collected', async () => {
    const bridges: EngineBridge[] = [];
    setWasmKernel(function (...args: ConstructorParameters<RustKernelClass>) {
      bridges.push(args[1]);
      return new WasmKernel(...args);
    } as unknown as RustKernelClass);
    try {
      const k = await openFixture();
      const [bridge] = bridges;
      const ready = new Promise<void>((done) => {
        const off = k.documents.onOpened((event) => {
          if (event.documentId !== 'second') return;
          off();
          done();
        });
      });
      k.documents.openAll([{ source: { kind: 'bytes', id: 'second', bytes }, scope: ['*'] }]);
      await ready;
      // In a function of its own, so nothing here outlives it.
      const render = async () => {
        const stage = k.capability(StageToken, 'second');
        stage.setViewportSize({ width: 800, height: 600 });
        const page = stage.listVisiblePages()[0].ref;
        await k.capability(RenderToken, 'second').renderPage(page, { width: 100, format: 'bmp' });
      };
      await render();
      await new Promise((r) => setTimeout(r, 0));
      const held = bridge.counts().hosts;
      expect(held).toBeGreaterThan(0);

      const release = vi.spyOn(bridge, 'release');
      await k.documents.close('second');
      expect(await collectUntil(() => release.mock.calls.length > 0)).toBe(true);
      await new Promise((r) => setTimeout(r, 0));
      expect(bridge.counts().hosts).toBeLessThan(held);
    } finally {
      setWasmKernel(WasmKernel);
    }
  });
});

describe('a destroyed kernel', () => {
  test('ignores the refs JavaScript drops after it is freed', async () => {
    const k = await openFixture();
    const stage = k.capability(StageToken);
    // Refs nothing keeps, still registered with the wire's finalizer.
    for (let i = 0; i < 50; i++) stage.onPageChanged(() => {});
    await k.destroy();
    kernel = undefined;

    const releaseRef = vi.spyOn(WasmKernel.prototype, 'release_ref');
    await collectUntil(() => false);
    expect(releaseRef).not.toHaveBeenCalled();
  });
});

describe("the wire's answer reuse", () => {
  test('releases the fresh refs of an answer it does not decode', () => {
    let next = 10;
    const released: number[] = [];
    const rust = {
      capability: () => '{"getThing":{"$ref":0,"pinned":true}}',
      invoke_ref: () => JSON.stringify({ returned: { size: 1, read: { $ref: next++ } } }),
      release_ref: (id: number) => released.push(id),
      // `capability` drains this first (`Wire.evict`); nothing here closes.
      take_evicted: () => '[]',
    } as unknown as RustKernel;
    const wire = new Wire({ host: () => undefined, hold: () => 0 } as unknown as EngineBridge);
    wire.attach(rust);
    const thing = wire.capability('thing') as { getThing(): { read: () => unknown } };

    const first = thing.getThing();
    wire.bump();
    // Same text but for the ref id: the first answer's object comes back,
    // and ref 11, which nothing decoded, is released on the spot.
    expect(thing.getThing()).toBe(first);
    expect(released).toEqual([11]);
  });
});
