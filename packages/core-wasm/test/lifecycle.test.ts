/**
 * The shim's `Kernel` lifecycle against the TS kernel's contract
 * (`packages/core/main/src/kernel.ts`), over the real `WasmKernel` and an
 * engine that is never asked for anything: no document opens here, so
 * these run without the PDFium runtime.
 */
import { afterEach, beforeAll, describe, expect, test, vi } from 'vitest';
import type { CapabilityToken, Engine, Kernel } from '@embedpdf/core';

import { WasmKernel } from '../wasm-node/epdf_web.js';
import { commandsPlugin, createKernel, kernelCounts, setWasmKernel } from '../src';

const DocumentsToken = { name: 'documents' } as CapabilityToken<unknown>;
const CommandsToken = { name: 'commands' } as CapabilityToken<unknown>;

let kernel: Kernel | undefined;

beforeAll(() => {
  setWasmKernel(WasmKernel);
});
afterEach(async () => {
  vi.restoreAllMocks();
  await kernel?.destroy();
  kernel = undefined;
});

function build(plugins: Parameters<typeof createKernel>[0]['plugins'] = [commandsPlugin()]) {
  const k = createKernel({ engine: {} as Engine, plugins });
  kernel = k;
  return k;
}

describe('a destroyed kernel', () => {
  test('answers as the TS kernel does', async () => {
    const k = build();
    await k.start();
    const held = k.getState();
    expect(held.plugins.commands).toBeDefined();
    await k.destroy();

    expect(k.status()).toBe('destroyed');
    expect(k.tryCapability(DocumentsToken)).toBe(null);
    expect(k.tryCapability(CommandsToken)).toBe(null);
    expect(() => k.capability(CommandsToken)).toThrow(
      '[kernel] capability("commands") on a destroyed kernel',
    );
    // The destroyed store: core reset, every slice gone.
    expect(k.getState()).toEqual({
      core: { documents: {}, pending: {}, order: [], activeId: null },
      plugins: {},
    });
    // A snapshot held from before reads its slices as gone, not throwing.
    expect(held.plugins.commands).toBeUndefined();
    await expect(k.start()).rejects.toThrow('[kernel] start() on a destroyed kernel');
  });

  test('stops serving cached capabilities as soon as destroy() is called', async () => {
    const k = build();
    await k.start();
    expect(k.tryCapability(DocumentsToken)).not.toBe(null);
    const destroying = k.destroy();
    // With no document to close the Rust side may already be through; the
    // shim frees it only once its promise settles, so it is still there.
    const now = k.status();
    expect(['destroying', 'destroyed']).toContain(now);
    expect(k.tryCapability(DocumentsToken)).toBe(null);
    await expect(k.start()).rejects.toThrow(`[kernel] start() on a ${now} kernel`);
    await destroying;
  });
});

describe('a kernel whose start failed', () => {
  test('no longer serves cached capabilities, and start() rejects again', async () => {
    const k = build();
    expect(k.tryCapability(CommandsToken)).not.toBe(null);
    const failure = { $error: { name: 'Error', message: 'init failed' } };
    vi.spyOn(WasmKernel.prototype, 'start').mockReturnValue(Promise.reject(failure));
    await expect(k.start()).rejects.toThrow('init failed');

    vi.spyOn(WasmKernel.prototype, 'status').mockReturnValue('failed');
    const asked = vi.spyOn(WasmKernel.prototype, 'try_capability');
    k.tryCapability(CommandsToken);
    k.tryCapability(CommandsToken);
    // Asked of the kernel both times, not served from the cache.
    expect(asked).toHaveBeenCalledTimes(2);
    await expect(k.start()).rejects.toThrow('[kernel] start() on a failed kernel');
  });
});

describe('init-only JS plugins', () => {
  type Ctx = Record<string, (...args: never[]) => unknown> & { doc: unknown };

  test("a workspace plugin's init runs inside start(), awaited", async () => {
    let k!: Kernel;
    const seen: string[] = [];
    const plugin = {
      id: 'late',
      init: async () => {
        seen.push(k.status());
        await new Promise((done) => setTimeout(done, 5));
        seen.push('init done');
      },
    };
    k = build([commandsPlugin(), plugin]);
    await k.start();
    seen.push('started');
    expect(seen).toEqual(['starting', 'init done', 'started']);
    expect(k.status()).toBe('started');
  });

  test('a failing init fails start() and the kernel', async () => {
    const torn: string[] = [];
    const plugin = {
      id: 'broken',
      init: async (ctx: { cleanup(fn: () => void): void }) => {
        ctx.cleanup(() => void torn.push('cleanup'));
        throw new Error('init failed');
      },
    };
    const k = build([commandsPlugin(), plugin]);
    await expect(k.start()).rejects.toThrow('init failed');
    expect(k.status()).toBe('failed');
    expect(torn).toEqual(['cleanup']);
    expect(k.tryCapability(CommandsToken)).toBe(null);
    expect(() => k.capability(CommandsToken)).toThrow(
      '[kernel] capability("commands") on a failed kernel',
    );
    await expect(k.start()).rejects.toThrow('[kernel] start() on a failed kernel');
  });

  test('the context has every PluginContext member', async () => {
    let ctx!: Ctx;
    const plugin = { id: 'probe', init: (c: Ctx) => void (ctx = c) };
    const k = build([commandsPlugin(), plugin]);
    await k.start();
    expect(ctx.getState()).toBeUndefined();
    expect(ctx.core()).toEqual(k.getState().core);
    expect(ctx.document()).toBe(null);
    expect(ctx.tryGet(CommandsToken as never)).toBe(k.tryCapability(CommandsToken));
    const off = ctx.subscribe((() => {}) as never);
    expect(typeof off).toBe('function');
    const refused = 'is not supported; only init-only JS plugins run in the eval build';
    expect(() => ctx.dispatch({ type: 'x' } as never)).toThrow(`ctx.dispatch ${refused}`);
    expect(() => ctx.documentHandle()).toThrow(`ctx.documentHandle ${refused}`);
    expect(() => ctx.doc).toThrow(`ctx.doc ${refused}`);
  });

  test('a rejected cleanup is reported, awaited by destroy, and a late one runs at once', async () => {
    const reported: unknown[] = [];
    let ctx!: { cleanup(fn: () => void | Promise<void>): void };
    const plugin = {
      id: 'tears',
      init: (c: typeof ctx) => {
        ctx = c;
        c.cleanup(() => Promise.reject(new Error('teardown failed')));
      },
    };
    const k = createKernel({
      engine: {} as Engine,
      plugins: [commandsPlugin(), plugin],
      report: (error) => reported.push(error),
    });
    kernel = k;
    await k.start();
    await k.destroy();
    expect(reported).toEqual([new Error('teardown failed')]);
    const late = vi.fn();
    ctx.cleanup(late);
    expect(late).toHaveBeenCalledOnce();
  });
});

describe('the documents capability', () => {
  test('kernel.documents is the proxy capability(DocumentsToken) answers', async () => {
    const k = build();
    await k.start();
    expect(k.capability(DocumentsToken)).toBe(k.documents);
    expect(k.tryCapability(DocumentsToken)).toBe(k.documents);
  });
});

describe('tokens by name', () => {
  test('an unknown token name is refused without adding a token', async () => {
    const k = build();
    await k.start();
    const before = kernelCounts(k).tokens;
    for (let i = 0; i < 20; i++) {
      const token = { name: `made-up-${i}` } as CapabilityToken<unknown>;
      expect(k.tryCapability(token)).toBe(null);
      expect(() => k.capability(token)).toThrow(`No capability "made-up-${i}".`);
    }
    expect(kernelCounts(k).tokens).toBe(before);
  });
});

describe('functions the kernel never calls', () => {
  /** Two tasks: the sweep's, and one more for an id a value carried. */
  const tasks = async () => {
    for (let i = 0; i < 2; i++) await new Promise((r) => setTimeout(r, 0));
  };

  test('a member Rust ignores is forgotten a task later; one it holds is kept', async () => {
    const k = build();
    await k.start();
    const commands = k.capability(CommandsToken) as {
      registerCommand(def: object): () => void;
    };
    await tasks();
    const start = kernelCounts(k).fns;
    const off = commands.registerCommand({
      id: 'probe:run',
      labelKey: 'probe',
      run: () => {},
      // Not a `CommandDef` member: the kernel reads past it.
      ignored: () => {},
    });
    expect(kernelCounts(k).fns).toBe(start + 2);
    await tasks();
    expect(kernelCounts(k).fns).toBe(start + 1);
    // `run` is still the kernel's to call.
    await expect(
      (k.capability(CommandsToken) as { execute(id: string): Promise<unknown> }).execute(
        'probe:run',
      ),
    ).resolves.toEqual({ status: 'executed' });
    off();
  });
});
