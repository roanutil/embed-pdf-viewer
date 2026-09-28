import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeAll, describe, expect, test, vi } from 'vitest';
import { createLocalEngine } from '@embedpdf/engine';
import type { Kernel } from '@embedpdf/core';
import { InteractionToken } from '@embedpdf/plugin-interaction';
import { StageToken } from '@embedpdf/plugin-stage';
import { CommandsToken } from '@embedpdf/plugin-commands';
import { ShellToken } from '@embedpdf/plugin-shell';
import { AnnotationToken } from '@embedpdf/plugin-annotation';
import { SearchToken } from '@embedpdf/plugin-search';

import { RenderToken } from '@embedpdf/plugin-render/contract/host';

import { WasmKernel } from '../wasm-node/epdf_web.js';
import {
  annotationPlugin,
  commandsPlugin,
  createKernel,
  interactionPlugin,
  kernelCounts,
  renderPlugin,
  searchPlugin,
  setHosts,
  setWasmKernel,
  shellPlugin,
  stagePlugin,
  type EngineBridge,
  type RustKernelClass,
} from '../src';
import { demoToolsPlugin } from '../../viewer/chrome/src/config/demo-tools.plugin';

const here = dirname(fileURLToPath(import.meta.url));
const fixtures = resolve(here, '..', '..', 'engine', 'main', 'test', 'fixtures');
/** Two pages (`/Count 2`). */
const FIXTURE = 'two_page_sigfield.pdf';

let bytes: Uint8Array;
let kernel: Kernel | undefined;

beforeAll(async () => {
  setWasmKernel(WasmKernel);
  bytes = new Uint8Array(await readFile(resolve(fixtures, FIXTURE)));
});
afterEach(async () => {
  await kernel?.destroy();
  kernel = undefined;
  setHosts({});
});

/** Start `plugins` over the local engine and open the fixture (or `file`). */
async function openFixture(
  plugins: Parameters<typeof createKernel>[0]['plugins'],
  file?: Uint8Array,
) {
  const engine = createLocalEngine({ runtime: { prefer: 'wasm' } });
  const k = createKernel({ engine, plugins });
  kernel = k;
  await k.start();
  const ready = new Promise<void>((done) => {
    const off = k.documents.onOpened(() => {
      off();
      done();
    });
  });
  k.documents.openAll([
    { source: { kind: 'bytes', id: 'fixture', bytes: file ?? bytes }, scope: ['*'] },
  ]);
  await ready;
  return k;
}

describe('the Kernel shim over the Rust kernel', () => {
  test('a getter answers the same object until the store changes', async () => {
    const k = await openFixture([stagePlugin(), interactionPlugin(), renderPlugin()]);
    const stage = k.capability(StageToken);
    expect(k.capability(StageToken, 'fixture')).toBe(stage);
    // 120 px tall, the fixture overflows, so a pan really moves the camera.
    stage.setViewportSize({ width: 800, height: 120 });

    const visible = stage.listVisiblePages();
    expect(visible.length).toBeGreaterThan(0);
    expect(stage.listVisiblePages()).toBe(visible);
    expect(typeof visible[0].transform.toPixels).toBe('function');
    const { viewScale } = visible[0].transform;

    // Proves the rehydration path: a `pageTransform(...)` rebuilt from the
    // page's own data runs `toPixels` in JavaScript, with no kernel call.
    const invokeRef = vi.spyOn(WasmKernel.prototype, 'invoke_ref');
    const callsBefore = invokeRef.mock.calls.length;
    expect(visible[0].transform.toPixels({ x: 10, y: 20 })).toEqual({
      x: 10 * viewScale,
      y: 20 * viewScale,
    });
    expect(invokeRef.mock.calls.length).toBe(callsBefore);
    invokeRef.mockRestore();

    stage.panBy(0, -100);
    expect(stage.listVisiblePages()).not.toBe(visible);
  });

  test('two getters read in turn keep answering the same objects', async () => {
    const k = await openFixture([shellPlugin()]);
    const shell = k.capability(ShellToken);
    shell.open('thumbnails', { props: { from: 'test' } });

    // A `useSyncExternalStore` snapshot per getter, as `useSurface` reads
    // `isOpen` and `getSurface(...).props` on every render: each getter's
    // real call must not cost the other its reference.
    const surface = shell.getSurface('thumbnails');
    expect(shell.isOpen('thumbnails')).toBe(true);
    expect(shell.getSurface('thumbnails')).toBe(surface);
    expect(shell.isOpen('thumbnails')).toBe(true);
    expect(shell.getSurface('thumbnails')).toBe(surface);

    shell.open('thumbnails', { props: { from: 'again' } });
    expect(shell.getSurface('thumbnails')).not.toBe(surface);
  });

  test("listVisiblePages keeps its object across another getter's call", async () => {
    const k = await openFixture([
      stagePlugin(),
      interactionPlugin(),
      renderPlugin(),
      shellPlugin(),
    ]);
    const stage = k.capability(StageToken);
    stage.setViewportSize({ width: 800, height: 600 });

    // `listVisiblePages` answers the same text at a fixed camera; the wire's
    // reuse of an answer whose fresh refs differ is `release.test.ts`'s.
    const visible = stage.listVisiblePages();
    k.capability(ShellToken).isOpen('sidebar');
    expect(stage.listVisiblePages()).toBe(visible);
  });

  test('listVisiblePages at a fixed camera registers no refs', async () => {
    const k = await openFixture([
      stagePlugin(),
      interactionPlugin(),
      renderPlugin(),
      shellPlugin(),
    ]);
    const stage = k.capability(StageToken);
    const shell = k.capability(ShellToken);
    stage.setViewportSize({ width: 800, height: 600 });
    expect(stage.listVisiblePages().length).toBeGreaterThan(0);

    // A real call between reads, so each `listVisiblePages` reaches the
    // kernel instead of the wire's result cache.
    const before = kernelCounts(k).refs;
    for (let i = 0; i < 100; i++) {
      shell.isOpen('sidebar');
      stage.listVisiblePages();
    }
    expect(kernelCounts(k).refs).toBe(before);
  });

  test('getState keeps its object, and every unchanged slice, across other calls', async () => {
    const k = await openFixture([
      stagePlugin(),
      interactionPlugin(),
      renderPlugin(),
      shellPlugin(),
    ]);
    const stage = k.capability(StageToken);
    stage.setViewportSize({ width: 800, height: 120 });

    // `useT` reads `getState().plugins.i18n` as a snapshot: a real getter
    // call from another component must not hand it a fresh object.
    const state = k.getState();
    stage.listVisiblePages();
    k.capability(ShellToken).isOpen('sidebar');
    expect(k.getState()).toBe(state);

    stage.panBy(0, -100);
    const after = k.getState();
    expect(after).not.toBe(state);
    expect(after.plugins['stage::fixture']).not.toBe(state.plugins['stage::fixture']);
    expect(after.plugins['shell::fixture']).toBeDefined();
    expect(after.plugins['shell::fixture']).toBe(state.plugins['shell::fixture']);
  });

  test('a call that changes the store is not answered from the cache', async () => {
    const hello = new Uint8Array(await readFile(resolve(fixtures, 'hello_world.pdf')));
    const k = await openFixture([stagePlugin(), interactionPlugin(), searchPlugin()], hello);
    const search = k.capability(SearchToken, 'fixture');
    const result = await search.search({ text: 'l' });
    expect(result.hitCount).toBeGreaterThan(2);

    // The same member with the same (no) arguments twice: each moves the
    // active hit, so the second must reach the kernel too.
    const first = search.nextHit();
    const second = search.nextHit();
    expect(first).not.toBeNull();
    expect(second).not.toBeNull();
    expect(second!.charStart).not.toBe(first!.charStart);
  });

  test('a getter inside a subscribe listener sees a mutation made in that listener', async () => {
    const k = await openFixture([stagePlugin(), interactionPlugin(), renderPlugin()]);
    const stage = k.capability(StageToken);
    stage.setViewportSize({ width: 800, height: 120 });

    // The Rust store defers a change notification made from inside a
    // change listener: a dispatch from there only sets `pending`
    // (`crates/epdf-kernel/src/store.rs:206-219`), so `bump` cannot wait
    // for the next `subscribe` firing. Both calls below run inside the one
    // `kernel.subscribe` listener the outer `panBy` triggers, so the cache
    // entry `listVisiblePages` makes right before the nested `panBy` must
    // not still be the one the getter answers with right after it.
    let seenBeforeMutation: unknown;
    let seenAfterMutation: unknown;
    const off = k.subscribe(() => {
      off();
      seenBeforeMutation = stage.listVisiblePages();
      stage.panBy(0, -50);
      seenAfterMutation = stage.listVisiblePages();
    });
    stage.panBy(0, -50);

    expect(seenBeforeMutation).not.toBeUndefined();
    expect(seenAfterMutation).not.toBe(seenBeforeMutation);
  });

  test('an init-only JS plugin registers through the Rust capabilities', async () => {
    const k = await openFixture([interactionPlugin(), demoToolsPlugin()]);
    const interaction = k.capability(InteractionToken);
    await expect.poll(() => interaction.hasTool('signature')).toBe(true);
    interaction.activateTool('signature');
    expect(interaction.getActiveToolId()).toBe('signature');
    expect(interaction.getActiveTool().id).toBe('signature');
  });

  test('a command run resolves c.tryGet through the ctx markers', async () => {
    let zoom: unknown = 'not run';
    const k = await openFixture([
      stagePlugin(),
      interactionPlugin(),
      commandsPlugin({
        commands: [
          {
            id: 'probe',
            labelKey: 'probe',
            run: (c) => {
              zoom = c.tryGet(StageToken)?.getZoomLevel();
            },
          },
        ],
      }),
    ]);
    const result = await k.capability(CommandsToken).execute('probe');
    expect(result).toEqual({ status: 'executed' });
    expect(typeof zoom).toBe('number');
  });

  test('matchStroke takes a KeyboardEvent-like stroke', async () => {
    const k = await openFixture([
      commandsPlugin({
        commands: [{ id: 'probe', labelKey: 'probe', shortcut: 'ctrl+=', run: () => {} }],
      }),
    ]);
    // `useCommandShortcuts` hands over the `KeyboardEvent` itself, whose
    // stroke members are prototype getters that no own-property walk sees.
    class Stroke {
      get key() {
        return '=';
      }
      get code() {
        return 'Equal';
      }
      get ctrlKey() {
        return true;
      }
      get metaKey() {
        return false;
      }
      get altKey() {
        return false;
      }
      get shiftKey() {
        return false;
      }
    }
    const commands = k.capability(CommandsToken) as unknown as {
      matchStroke(stroke: unknown, options: { isMac: boolean }): string | null;
    };
    expect(commands.matchStroke(new Stroke(), { isMac: false })).toBe('probe');
  });

  test("the hosts' scheduler runs the stage's frames", async () => {
    const frames: ((timestamp: number) => void)[] = [];
    const raf = vi.fn((frame: (timestamp: number) => void) => frames.push(frame));
    setHosts({ scheduler: { raf, caf: vi.fn() } });
    const k = await openFixture([stagePlugin(), interactionPlugin(), renderPlugin()]);
    const stage = k.capability(StageToken);
    stage.setViewportSize({ width: 800, height: 120 });

    stage.scrollTo({ top: 100, behavior: 'smooth' });
    expect(raf).toHaveBeenCalled();
    for (let t = 0; t < 2000 && frames.length > 0; t += 16) frames.shift()!(t);
    expect(stage.getScrollMetrics().scrollTop).toBe(100);
  });

  test("the hosts' randomUuid names a new annotation", async () => {
    const randomUuid = vi.fn(() => '5a1d2c3e-0000-4000-8000-00000000abcd');
    setHosts({ annotation: { randomUuid } });
    const k = await openFixture([
      stagePlugin(),
      interactionPlugin(),
      renderPlugin(),
      annotationPlugin(),
    ]);
    const stage = k.capability(StageToken);
    stage.setViewportSize({ width: 800, height: 600 });
    const page = stage.listVisiblePages()[0].ref;
    await k.capability(AnnotationToken).create({
      page,
      subtype: 'square',
      bounds: { x: 10, y: 10, width: 50, height: 50 },
    });
    expect(randomUuid).toHaveBeenCalled();
  });

  test('a JS plugin with its own reducer is refused', () => {
    const engine = createLocalEngine({ runtime: { prefer: 'wasm' } });
    const plugin = { id: 'mine', reduce: (state: unknown) => state };
    expect(() => createKernel({ engine, plugins: [plugin] })).toThrow(
      '[core-wasm] JS plugin "mine" has reduce; only init-only JS plugins run in the eval build',
    );
  });
});

/** Open `id` (the fixture's bytes) in a started kernel and wait for it. */
async function openDocument(k: Kernel, id: string): Promise<void> {
  const ready = new Promise<void>((done) => {
    const off = k.documents.onOpened((event) => {
      if (event.documentId !== id) return;
      off();
      done();
    });
  });
  k.documents.openAll([{ source: { kind: 'bytes', id, bytes }, scope: ['*'] }]);
  await ready;
}

describe('closed documents and destroyed kernels', () => {
  test("a closed document's capabilities leave the memo and the proxy cache", async () => {
    const k = await openFixture([stagePlugin(), interactionPlugin(), renderPlugin()]);
    k.capability(StageToken);
    const before = kernelCounts(k);

    await openDocument(k, 'second');
    const stage = k.capability(StageToken, 'second');
    expect(k.capability(StageToken, 'second')).toBe(stage);
    const open = kernelCounts(k);
    expect(open.described).toBe(before.described + 1);
    expect(open.proxies).toBe(before.proxies + 1);

    await k.documents.close('second');
    const closed = kernelCounts(k);
    expect(closed.described).toBe(before.described);
    expect(closed.proxies).toBe(before.proxies);
    // The open document's capability is still the one it was.
    expect(k.capability(StageToken)).toBe(k.capability(StageToken, 'fixture'));
  });

  test('a destroyed kernel throws its own error on every later call', async () => {
    const k = await openFixture([stagePlugin(), interactionPlugin(), renderPlugin()]);
    await k.destroy();
    const destroyed = '[core-wasm] kernel destroyed';
    expect(() => k.capability(StageToken)).toThrow(destroyed);
    expect(() => k.tryCapability(StageToken)).toThrow(destroyed);
    expect(() => k.status()).toThrow(destroyed);
    expect(() => k.scopeOf(StageToken)).toThrow(destroyed);
    expect(() => k.getState()).toThrow(destroyed);
    expect(() => kernelCounts(k)).toThrow(destroyed);
    await k.destroy();
  });

  test('destroying one of two kernels frees its hosts and leaves the other working', async () => {
    const bridges: EngineBridge[] = [];
    const Capturing = function (...args: ConstructorParameters<RustKernelClass>) {
      bridges.push(args[1]);
      return new WasmKernel(...args);
    } as unknown as RustKernelClass;
    setWasmKernel(Capturing);
    try {
      const plugins = () => [stagePlugin(), interactionPlugin(), renderPlugin()];
      const other = await openFixture(plugins());
      const k = await openFixture(plugins());
      const [, bridge] = bridges;
      const stage = k.capability(StageToken);
      stage.setViewportSize({ width: 800, height: 600 });
      const page = stage.listVisiblePages()[0].ref;
      await k.capability(RenderToken).renderPage(page, { width: 100, format: 'bmp' });
      expect(bridge.counts().hosts).toBeGreaterThan(0);

      const release = vi.spyOn(bridge, 'release');
      await k.destroy();
      // The raster store's images went back to the bridge as the kernel
      // went: nothing in Rust kept the store alive past `free()`.
      expect(release).toHaveBeenCalled();
      expect(bridge.counts().hosts).toBe(0);
      const counts = kernelCounts(other);
      expect(counts.allHosts).toBe(counts.hosts);

      kernel = other;
      const otherStage = other.capability(StageToken);
      otherStage.setViewportSize({ width: 800, height: 600 });
      const otherPage = otherStage.listVisiblePages()[0].ref;
      const image = await other.capability(RenderToken).renderPage(otherPage, {
        width: 100,
        format: 'bmp',
      });
      expect(image).toBeDefined();
    } finally {
      setWasmKernel(WasmKernel);
    }
  });
});
