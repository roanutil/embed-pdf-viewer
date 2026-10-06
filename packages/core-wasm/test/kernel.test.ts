import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeAll, describe, expect, test, vi } from 'vitest';
import { createLocalEngine } from '@embedpdf/engine';
import { isCancelled, isPluginError, type Kernel, type PluginError } from '@embedpdf/core';
import { InteractionToken } from '@embedpdf/plugin-interaction';
import { StageToken } from '@embedpdf/plugin-stage';
import { CommandsToken } from '@embedpdf/plugin-commands';
import { ShellToken } from '@embedpdf/plugin-shell';
import { AnnotationToken } from '@embedpdf/plugin-annotation';
import { SearchToken } from '@embedpdf/plugin-search';

import { RenderToken } from '@embedpdf/plugin-render/contract/host';

import { WasmKernel, set_host_bytes_source, set_host_release } from '../wasm-node/epdf_web.js';
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
  toError,
} from '../src';
import { hostBytes, releaseHost } from '../src/engine-bridge';
import { demoToolsPlugin } from '../../viewer/chrome/src/config/demo-tools.plugin';

const here = dirname(fileURLToPath(import.meta.url));
const fixtures = resolve(here, '..', '..', 'engine', 'main', 'test', 'fixtures');
/** Two pages (`/Count 2`). */
const FIXTURE = 'two_page_sigfield.pdf';

let bytes: Uint8Array;
let kernel: Kernel | undefined;

beforeAll(async () => {
  setWasmKernel(WasmKernel);
  // As `initWasm()` installs them in the browser.
  set_host_bytes_source(hostBytes);
  set_host_release(releaseHost);
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

/** Past the next store notification: listeners hear of changes at most once
 *  a frame (`FRAME_MS` in `src/kernel.ts`), then a frame passes quietly. */
const notified = () => new Promise((resolve) => setTimeout(resolve, 40));

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
    // call from another component must not hand it a fresh object. Slices
    // are read when accessed, so read them while the state is current.
    const state = k.getState();
    const stageBefore = state.plugins['stage::fixture'];
    const shellBefore = state.plugins['shell::fixture'];
    stage.listVisiblePages();
    k.capability(ShellToken).isOpen('sidebar');
    expect(k.getState()).toBe(state);

    stage.panBy(0, -100);
    const after = k.getState();
    expect(after).not.toBe(state);
    expect(after.plugins['stage::fixture']).not.toBe(stageBefore);
    expect(after.plugins['shell::fixture']).toBeDefined();
    expect(after.plugins['shell::fixture']).toBe(shellBefore);
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
    await notified();

    expect(seenBeforeMutation).not.toBeUndefined();
    expect(seenAfterMutation).not.toBe(seenBeforeMutation);
  });

  test('a burst of store changes in one call notifies each listener once, after it returns', async () => {
    const k = await openFixture([stagePlugin(), interactionPlugin(), renderPlugin()]);
    const stage = k.capability(StageToken);
    stage.setViewportSize({ width: 800, height: 120 });
    await notified();

    let calls = 0;
    const off = k.subscribe(() => calls++);
    const before = k.getState();
    stage.panBy(0, -50);
    stage.panBy(0, -50);
    expect(calls).toBe(0);
    // The caches don't wait for the notification.
    expect(k.getState()).not.toBe(before);
    await notified();
    expect(calls).toBe(1);
    off();
  });

  test('a change after a quiet frame notifies in a microtask; the rest of the frame waits', async () => {
    const k = await openFixture([stagePlugin(), interactionPlugin(), renderPlugin()]);
    const stage = k.capability(StageToken);
    stage.setViewportSize({ width: 800, height: 120 });
    await notified();

    let calls = 0;
    const off = k.subscribe(() => calls++);
    stage.panBy(0, -10);
    await new Promise<void>((resolve) => queueMicrotask(resolve));
    expect(calls).toBe(1);
    // Five more changes, each in a task of its own, inside the same frame:
    // one notification, at the frame's end.
    for (let i = 0; i < 5; i++) {
      await new Promise((resolve) => setTimeout(resolve, 0));
      stage.panBy(0, -10);
    }
    expect(calls).toBe(1);
    await notified();
    expect(calls).toBe(2);
    off();
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

  test('the active hit is the same object as its entry in listHits', async () => {
    const hello = new Uint8Array(await readFile(resolve(fixtures, 'hello_world.pdf')));
    const k = await openFixture([stagePlugin(), interactionPlugin(), searchPlugin()], hello);
    const search = k.capability(SearchToken, 'fixture');
    await search.search({ text: 'l' });
    const next = search.nextHit();
    const active = search.getActiveHit();
    expect(active).not.toBeNull();
    const listed = search.listHits({ page: active!.page });
    expect(listed).toContain(active);
    expect(listed).toContain(next);
    expect(listed).toContain(search.goToHit(0));
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

  test("the annotation capability's comments and links are objects of functions", async () => {
    const k = await openFixture([
      stagePlugin(),
      interactionPlugin(),
      renderPlugin(),
      annotationPlugin(),
    ]);
    const stage = k.capability(StageToken);
    stage.setViewportSize({ width: 800, height: 600 });
    const page = stage.listVisiblePages()[0].ref;
    const annotation = k.capability(AnnotationToken);
    expect(annotation.comments.listThreads()).toEqual([]);

    const root = await annotation.create({
      page,
      subtype: 'square',
      contents: 'root',
      bounds: { x: 10, y: 10, width: 20, height: 20 },
    });
    const threads = annotation.comments.listThreads();
    expect(threads).toHaveLength(1);
    expect(annotation.comments.listThreads()).toBe(threads);

    const changed = vi.fn();
    const off = annotation.comments.onThreadChanged(changed);
    const reply = await annotation.comments.reply(root, 'agreed');
    off();
    expect(reply).toBeTruthy();
    expect(changed).toHaveBeenCalledTimes(1);
    expect(annotation.comments.getThread(root)?.replies).toHaveLength(1);
    expect(annotation.links.get(root)).toBe(null);
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
      // `bridge.dispose()` clears every host this bridge owns on its own,
      // so a bare `counts().hosts === 0` after destroy would hold even if
      // the raster store released nothing. Stub it out, so the only way
      // the count below can reach zero is a real `release` per held id (an
      // image's release also frees its paired `$hostBytes` id, so this can
      // be fewer calls than ids: `release`'s own tests cover that).
      const dispose = vi.spyOn(bridge, 'dispose').mockImplementation(() => {});
      await k.destroy();
      // `release` marks an id and deletes it on the next macrotask.
      await new Promise((r) => setTimeout(r, 0));
      // The raster store's images went back to the bridge as the kernel
      // went: nothing in Rust kept the store alive past `free()`.
      expect(release).toHaveBeenCalled();
      expect(bridge.counts().hosts).toBe(0);
      dispose.mockRestore();
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

describe('Rust rejections keep their error class', () => {
  test('toError rebuilds a PluginError, decoding its cause and details', () => {
    const error = toError({
      $error: {
        name: 'PluginError',
        message: 'no such page',
        code: 'not-found',
        capability: 'render',
        details: { page: { $number: 'NaN' }, missing: { $undefined: true } },
        cause: { $error: { name: 'CancelledError', message: '[kernel] cancelled: open' } },
      },
    });
    expect(isPluginError(error, 'not-found')).toBe(true);
    expect(error.message).toBe('no such page');
    expect((error as PluginError).capability).toBe('render');
    expect((error as PluginError).details).toEqual({ page: NaN });
    const cause = (error as { cause?: unknown }).cause;
    expect(isCancelled(cause)).toBe(true);
    expect((cause as Error).message).toBe('[kernel] cancelled: open');
  });

  test('closing a document while it opens rejects the open with a CancelledError', async () => {
    const engine = createLocalEngine({ runtime: { prefer: 'wasm' } });
    const k = createKernel({ engine, plugins: [stagePlugin()] });
    kernel = k;
    await k.start();
    let release!: () => void;
    const gate = new Promise<void>((done) => (release = done));
    const opening = k.documents.open(async () => {
      await gate;
      return { kind: 'bytes', id: 'late', bytes };
    });
    const [id] = k.documents.list().map((d) => d.id);
    const settled = opening.then(
      () => null,
      (error: unknown) => error,
    );
    await k.documents.close(id);
    release();
    const error = await settled;
    expect(isCancelled(error)).toBe(true);
  });
});
