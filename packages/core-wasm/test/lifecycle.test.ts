/**
 * The shim's `Kernel` lifecycle against the TS kernel's contract
 * (`packages/core/main/src/kernel.ts`), over the real `WasmKernel` and an
 * engine that is never asked for anything: no document opens here, so
 * these run without the PDFium runtime.
 */
import { afterEach, beforeAll, describe, expect, test, vi } from 'vitest';
import type { CapabilityToken, Engine, Kernel } from '@embedpdf/core';

import { WasmKernel } from '../wasm-node/epdf_web.js';
import { commandsPlugin, createKernel, setWasmKernel } from '../src';

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
