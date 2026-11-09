/**
 * A `{ kind: 'url' }` source over the Rust kernel: downloaded under the
 * loading tab with the host's `fetch` (else the global one), as the
 * TypeScript kernel's `fetchUrlSource` does
 * (`packages/core/main/src/kernel.ts`), then opened as bytes.
 */
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeAll, describe, expect, test, vi } from 'vitest';
import { createLocalEngine } from '@embedpdf/engine';
import { isPluginError, type HostFetch, type Kernel } from '@embedpdf/core';

import { WasmKernel, set_host_bytes_source, set_host_release } from '../wasm-node/epdf_web.js';
import {
  createKernel,
  interactionPlugin,
  kernelCounts,
  renderPlugin,
  setWasmKernel,
  stagePlugin,
} from '../src';
import { hostBytes, releaseHost } from '../src/engine-bridge';

const here = dirname(fileURLToPath(import.meta.url));
const fixtures = resolve(here, '..', '..', 'engine', 'main', 'test', 'fixtures');
/** Two pages (`/Count 2`). */
const FIXTURE = 'two_page_sigfield.pdf';
const URL = 'https://files.example/two-pages.pdf';

let pdf: Uint8Array;
let kernel: Kernel | undefined;

beforeAll(async () => {
  setWasmKernel(WasmKernel);
  set_host_bytes_source(hostBytes);
  set_host_release(releaseHost);
  pdf = new Uint8Array(await readFile(resolve(fixtures, FIXTURE)));
});
afterEach(async () => {
  await kernel?.destroy();
  kernel = undefined;
  vi.unstubAllGlobals();
});

/** The bridge's held values, once a release's macrotask has run. */
const hosts = async (k: Kernel) => {
  await new Promise((done) => setTimeout(done, 0));
  return kernelCounts(k).hosts;
};

const answer = (status: number, body: Uint8Array) => ({
  ok: status >= 200 && status < 300,
  status,
  arrayBuffer: async () => body.slice().buffer,
});

async function started(fetch?: HostFetch): Promise<Kernel> {
  const engine = createLocalEngine({ runtime: { prefer: 'wasm' } });
  const k = createKernel({
    engine,
    plugins: [stagePlugin(), interactionPlugin(), renderPlugin()],
    ...(fetch ? { fetch } : {}),
  });
  kernel = k;
  await k.start();
  return k;
}

async function refusal(promise: Promise<unknown>): Promise<unknown> {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error('expected a refusal');
}

describe('a URL source', () => {
  test("downloads with the host's fetch, under the tab's signal, and opens as bytes", async () => {
    const fetch = vi.fn<HostFetch>(async () => answer(200, pdf));
    const k = await started(fetch);
    const before = await hosts(k);
    const result = await k.documents.open({ kind: 'url', url: URL, id: 'remote' });
    expect(result.document).toMatchObject({ id: 'remote', status: 'ready', pageCount: 2 });
    expect(fetch).toHaveBeenCalledTimes(1);
    const [url, init] = fetch.mock.calls[0];
    expect(url).toBe(URL);
    expect(init.signal).toBeInstanceOf(AbortSignal);
    expect(init.signal.aborted).toBe(false);
    // The file is held on this side while the document is open, and let go with it.
    expect(await hosts(k)).toBeGreaterThan(before);
    await k.documents.close('remote');
    expect(await hosts(k)).toBe(before);
  });

  test('downloads with the global fetch when the host gives none', async () => {
    const global = vi.fn(async () => answer(200, pdf));
    vi.stubGlobal('fetch', global);
    const k = await started();
    const result = await k.documents.open({ kind: 'url', url: URL });
    expect(result.document).toMatchObject({ status: 'ready', pageCount: 2 });
    expect(global).toHaveBeenCalledWith(
      URL,
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  test("an HTTP refusal fails the tab with the status's code", async () => {
    const cases = [
      [404, 'not-found'],
      [403, 'permission-denied'],
      [401, 'permission-denied'],
      [500, 'operation-failed'],
    ] as const;
    for (const [status, code] of cases) {
      await kernel?.destroy();
      const next = await started(async () => answer(status, new Uint8Array()));
      const error = await refusal(next.documents.open({ kind: 'url', url: URL, id: `s${status}` }));
      expect(isPluginError(error)).toBe(true);
      expect(error).toMatchObject({
        code,
        capability: 'documents',
        message: `couldn't download ${URL} (HTTP ${status})`,
      });
      expect(next.getState().core.documents[`s${status}`]?.status ?? 'gone').not.toBe('ready');
    }
  });

  test("a file that isn't a PDF fails invalid-input, and its bytes go back to the host", async () => {
    const page = new TextEncoder().encode('<html>sign in</html>');
    const k = await started(async () => answer(200, page));
    const before = await hosts(k);
    const error = await refusal(k.documents.open({ kind: 'url', url: URL }));
    expect(await hosts(k)).toBe(before);
    expect(isPluginError(error)).toBe(true);
    expect(error).toMatchObject({ code: 'invalid-input', message: `${URL} isn't a PDF` });
  });

  test('a fetch that rejects fails the tab with its message', async () => {
    const k = await started(async () => {
      throw new TypeError('Failed to fetch');
    });
    const error = await refusal(k.documents.open({ kind: 'url', url: URL }));
    // As any failed open: a PluginError with the cause's message.
    expect(isPluginError(error)).toBe(true);
    expect(error).toMatchObject({ message: 'Failed to fetch' });
  });

  test('closing the tab while it downloads aborts the signal fetch was given', async () => {
    let signal: AbortSignal | undefined;
    const k = await started(
      (_url, init) =>
        new Promise((_, reject) => {
          signal = init.signal;
          init.signal.addEventListener('abort', () => reject(init.signal.reason));
        }),
    );
    const opening = k.documents.open({ kind: 'url', url: URL, id: 'slow' });
    opening.catch(() => {});
    await vi.waitFor(() => expect(signal).toBeDefined());
    await k.documents.close('slow');
    expect(signal!.aborted).toBe(true);
    const error = await refusal(opening);
    expect(isPluginError(error)).toBe(true);
    expect(error).toMatchObject({ code: 'operation-cancelled' });
  });
});
