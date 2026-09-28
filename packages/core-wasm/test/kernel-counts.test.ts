/**
 * `kernelCounts`'s default-kernel resolution: it must fail loudly rather
 * than silently report a destroyed kernel or an arbitrary one of several
 * live kernels. Kept in its own file (rather than `kernel.test.ts`) so
 * these assertions about `liveKernels` being empty or having exactly one
 * member don't depend on every other test in a shared file cleaning up
 * after itself in the right order.
 */
import { afterEach, beforeAll, describe, expect, test } from 'vitest';
import { createLocalEngine } from '@embedpdf/engine';
import type { Kernel } from '@embedpdf/core';

import { WasmKernel } from '../wasm-node/epdf_web.js';
import { createKernel, kernelCounts, setWasmKernel } from '../src';

beforeAll(() => {
  setWasmKernel(WasmKernel);
});

/** A kernel with no document opened: `kernelCounts` only reads the
 *  harness/bridge/wire tables, none of which need one. */
async function bareKernel(): Promise<Kernel> {
  const engine = createLocalEngine({ runtime: { prefer: 'wasm' } });
  const k = createKernel({ engine, plugins: [] });
  await k.start();
  return k;
}

let kernels: Kernel[] = [];
afterEach(async () => {
  await Promise.all(kernels.map((k) => k.destroy()));
  kernels = [];
});

describe('kernelCounts default resolution', () => {
  test('throws when no kernel is live', () => {
    expect(() => kernelCounts()).toThrow('[core-wasm] kernelCounts: no live kernel');
  });

  test('defaults to the sole live kernel', async () => {
    const k = await bareKernel();
    kernels.push(k);
    expect(kernelCounts()).toEqual(kernelCounts(k));
  });

  test('throws once a second kernel is live, naming the count', async () => {
    const a = await bareKernel();
    const b = await bareKernel();
    kernels.push(a, b);
    expect(() => kernelCounts()).toThrow('[core-wasm] kernelCounts: 2 live kernels');
    // Each one is still reachable by explicit reference.
    expect(() => kernelCounts(a)).not.toThrow();
    expect(() => kernelCounts(b)).not.toThrow();
  });

  test('a destroyed kernel drops out of the default and cannot be read from at all', async () => {
    const k = await bareKernel();
    await k.destroy();
    expect(() => kernelCounts()).toThrow('[core-wasm] kernelCounts: no live kernel');
  });

  test('destroying one of two live kernels leaves the other as the sole default', async () => {
    const a = await bareKernel();
    const b = await bareKernel();
    kernels.push(b);
    await a.destroy();
    expect(kernelCounts()).toEqual(kernelCounts(b));
  });
});
