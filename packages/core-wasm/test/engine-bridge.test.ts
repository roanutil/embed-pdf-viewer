import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, afterEach, beforeAll, describe, expect, test, vi } from 'vitest';
import { createLocalEngine } from '@embedpdf/engine';
import type { Engine } from '@embedpdf/engine-core/runtime';

import {
  canonical,
  canonicalText,
  isPlainJson,
  createEngineBridge,
  decode,
  encodeInput,
  errorSpec,
  hostBytes,
  releaseHost,
  type EngineBridge,
} from '../src/engine-bridge';

const here = dirname(fileURLToPath(import.meta.url));
const fixtures = resolve(here, '..', '..', 'engine', 'main', 'test', 'fixtures');
/** Two pages (`/Count 2`). */
const FIXTURE = 'two_page_sigfield.pdf';

const bytesJson = (bytes: Uint8Array) => ({ $bytes: Buffer.from(bytes).toString('base64') });

describe('the engine bridge over the local engine', () => {
  let engine: Engine;
  let bridge: EngineBridge;
  let docId: string;
  let firstPon: number;
  let pagesList: { pageCount: number; pages: unknown[] };

  beforeAll(async () => {
    engine = createLocalEngine({ runtime: { prefer: 'wasm' } });
    bridge = createEngineBridge(engine);
    const bytes = new Uint8Array(await readFile(resolve(fixtures, FIXTURE)));
    docId = JSON.parse(
      await bridge.open(
        JSON.stringify({ kind: 'bytes', id: 'fixture', bytes: bytesJson(bytes) }),
        JSON.stringify({ scope: ['*'] }),
      ),
    );
    pagesList = JSON.parse(await bridge.call(docId, 'pages.list', null, '[]'));
    firstPon = (pagesList.pages[0] as { ref: { pageObjectNumber: number } }).ref.pageObjectNumber;
  });
  afterAll(async () => {
    await bridge.destroy();
  });

  test('open answers the handle id', () => {
    expect(docId).toBe('fixture');
  });

  test('pages.list answers the fixture page count as canonical JSON', () => {
    expect(pagesList.pageCount).toBe(2);
    expect(pagesList.pages).toHaveLength(2);
    expect(typeof firstPon).toBe('number');
  });

  test('a page render answers a byte count, not the bytes, with a $host key for the live handle', async () => {
    const json = await bridge.call(
      docId,
      'page.render.image',
      firstPon,
      JSON.stringify([{ width: 64, format: 'bmp' }]),
    );
    expect(json).not.toContain('$bytes');
    const answer = JSON.parse(json);
    expect(answer.format).toBe('bmp');
    expect(answer.source.kind).toBe('bytes');
    expect(typeof answer.$host).toBe('number');
    const live = bridge.host(answer.$host) as {
      objectUrl?: unknown;
      source: { bytes: Uint8Array };
    };
    expect(answer.source.bytes.$byteLength).toBe(live.source.bytes.byteLength);
    expect(typeof live.objectUrl).toBe('function');
    expect(typeof answer.source.bytes.$hostBytes).toBe('number');
    expect(bridge.hostBytes(answer.source.bytes.$hostBytes)).toEqual(live.source.bytes);
  });

  test('synchronous reads: capabilities, security, has, pageIndex', () => {
    expect(JSON.parse(bridge.capabilities(docId))).toHaveProperty('pageEditSessions');
    expect(JSON.parse(bridge.security(docId, 'allows', '["doc.render"]'))).toBe(true);
    expect(JSON.parse(bridge.security(docId, 'passwordPrompt', '[]'))).toEqual({ state: 'none' });
    expect(JSON.parse(bridge.security(docId, 'current', '[]'))).toHaveProperty('encryption');
    expect(bridge.has(docId, 'render')).toBe(true);
    expect(bridge.has(docId, 'pages.flatten')).toBe(true);
    expect(bridge.has(docId, 'page.measure')).toBe(true);
    expect(bridge.has(docId, 'noSuchService')).toBe(false);
    expect(bridge.has(docId, 'page.noSuchService')).toBe(false);
    expect(bridge.pageIndex(docId, firstPon)).toBe(-1);
    expect(bridge.lastServerId(docId)).toBeNull();
  });

  test('an engine refusal rejects with the canonical error', async () => {
    const refused = bridge.call(docId, 'pages.noSuchMethod', null, '[]');
    await expect(refused).rejects.toEqual({
      $error: expect.objectContaining({ name: 'TypeError' }),
    });
    const missing = bridge.call('nope', 'pages.list', null, '[]');
    await expect(missing).rejects.toEqual({
      $error: expect.objectContaining({ message: '[engine-bridge] no document "nope"' }),
    });
  });

  test('a call can be aborted', async () => {
    const call = bridge.call(
      docId,
      'page.render.image',
      firstPon,
      JSON.stringify([{ width: 64, format: 'bmp' }]),
    );
    call.abort?.('stop');
    await expect(call).rejects.toEqual({ $error: expect.objectContaining({ name: 'AbortError' }) });
  });

  test('subscribe hands events over as canonical JSON, and unsubscribes', async () => {
    const events: unknown[] = [];
    const off = bridge.subscribe(docId, (json) => events.push(JSON.parse(json)));
    await bridge.call(docId, 'metadata.update', null, JSON.stringify([{ title: 'Bridged' }]));
    expect(events.length).toBeGreaterThan(0);
    off();
    const seen = events.length;
    await bridge.call(docId, 'metadata.update', null, JSON.stringify([{ title: 'Again' }]));
    expect(events.length).toBe(seen);
  });

  test('close forgets the document once it settles', async () => {
    expect(bridge.counts().documents).toBe(1);
    expect(JSON.parse(await bridge.call(docId, 'close', null, '[]'))).toEqual({ $undefined: true });
    // The handle is dropped from the table, so a closed document no longer
    // lives until the kernel is destroyed; a later call on its id answers
    // the bridge's own unknown-document refusal.
    expect(bridge.counts().documents).toBe(0);
    await expect(bridge.call(docId, 'pages.list', null, '[]')).rejects.toEqual({
      $error: expect.objectContaining({ message: `[engine-bridge] no document "${docId}"` }),
    });
  });

  test('each open and close leaves no document behind', async () => {
    const bytes = new Uint8Array(await readFile(resolve(fixtures, FIXTURE)));
    for (const id of ['again-1', 'again-2']) {
      const opened = JSON.parse(
        await bridge.open(JSON.stringify({ kind: 'bytes', id, bytes: bytesJson(bytes) })),
      );
      expect(bridge.counts().documents).toBe(1);
      await bridge.call(opened, 'close', null, '[]');
      expect(bridge.counts().documents).toBe(0);
    }
  });
});

describe('two bridges share one host table', () => {
  test('ids are unique across bridges, and the module-level hostBytes resolves either', () => {
    // Two independent bridges, as two live `WasmKernel`s would each build
    // one (two viewers on a page, or a remount that has not torn the first
    // one down yet). Neither `engine` argument is used by `hold`/`host`.
    const bridgeA = createEngineBridge({} as Engine);
    const bridgeB = createEngineBridge({} as Engine);
    const idA = bridgeA.hold(new Uint8Array([1, 2, 3]));
    const idB = bridgeB.hold(new Uint8Array([4, 5, 6, 7]));

    expect(idA).not.toBe(idB);
    // The module-level function, what a Rust resolver installed once for
    // the whole wasm instance actually calls: it resolves either bridge's
    // id, regardless of which bridge made it or which was built last.
    expect(hostBytes(idA)).toEqual(new Uint8Array([1, 2, 3]));
    expect(hostBytes(idB)).toEqual(new Uint8Array([4, 5, 6, 7]));
    // Each bridge's own `hostBytes`/`host` stay scoped to what it made.
    expect(bridgeA.hostBytes(idA)).toEqual(new Uint8Array([1, 2, 3]));
    expect(() => bridgeA.hostBytes(idB)).toThrow(/no host value/);
    expect(() => bridgeA.host(idB)).toThrow(/no host value/);
    expect(bridgeA.counts().hosts).toBe(1);
    expect(bridgeB.counts().hosts).toBe(1);
  });

  test('hostBytes throws for an id nothing holds, or one that is not bytes', () => {
    const bridge = createEngineBridge({} as Engine);
    const notBytes = bridge.hold({ not: 'bytes' });
    expect(() => hostBytes(notBytes)).toThrow('[engine-bridge] no host bytes');
    expect(() => hostBytes(999999)).toThrow('[engine-bridge] no host bytes');
  });
});

/** A rendered image as the engine hands one back: a class instance (so it
 *  crosses as `$host`) whose `source` carries its encoded bytes. */
class FakeImage {
  source = { kind: 'bytes' as const, bytes: new Uint8Array([1, 2, 3]) };
  width = 1;
  height = 1;
  objectUrl(): string {
    return 'blob:fake';
  }
}

/** Just enough of an `Engine` for `open` and `page.render.image`. */
function imageEngine(): Engine {
  const doc = {
    id: 'doc',
    page: () => ({ render: { image: async () => new FakeImage() } }),
  };
  return { open: async () => doc } as unknown as Engine;
}

/** Render one image through `bridge`: its canonical JSON text, `$host` id
 *  and `$hostBytes` id. */
async function renderOne(bridge: EngineBridge) {
  await bridge.open(JSON.stringify('input'));
  const json = await bridge.call('doc', 'page.render.image', 1, '[]');
  const answer = JSON.parse(json) as { $host: number; source: { bytes: { $hostBytes: number } } };
  return { json, n: answer.$host, m: answer.source.bytes.$hostBytes };
}

const macrotask = () => new Promise((done) => setTimeout(done, 0));

describe('release', () => {
  const errors = vi.spyOn(console, 'error').mockImplementation(() => {});
  afterEach(() => errors.mockClear());
  afterAll(() => errors.mockRestore());

  test('marks the image and its bytes now, and deletes both on the next macrotask', async () => {
    const bridge = createEngineBridge(imageEngine());
    const { n, m } = await renderOne(bridge);
    expect(bridge.counts().hosts).toBe(2);
    const shared = bridge.counts().allHosts;

    bridge.release(n);
    // Marked, not yet deleted: both still resolve, and the counts still
    // count them.
    expect(bridge.host(n)).toBeInstanceOf(FakeImage);
    expect(hostBytes(m)).toEqual(new Uint8Array([1, 2, 3]));
    expect(bridge.counts()).toMatchObject({ hosts: 2, allHosts: shared });

    await macrotask();
    expect(bridge.counts()).toMatchObject({ hosts: 0, allHosts: shared - 2 });
    expect(() => bridge.host(n)).toThrow(`[engine-bridge] host value ${n} was released`);
    expect(() => bridge.hostBytes(m)).toThrow(`[engine-bridge] host value ${m} was released`);
    expect(() => hostBytes(m)).toThrow(`[engine-bridge] host bytes ${m} was released`);
    // A released id that comes back is reported loudly, not only thrown.
    expect(errors).toHaveBeenCalledTimes(3);
  });

  test('a decode queued before the release still gets the image', async () => {
    // The race `runtime::to_js` sets up: Rust resolves the promise with the
    // image's JSON, and drops its last handle, in one entry; the shim's
    // decode is a `.then` reaction that runs after.
    const bridge = createEngineBridge(imageEngine());
    const { json, n } = await renderOne(bridge);
    let resolve!: (text: string) => void;
    const promise = new Promise<string>((done) => (resolve = done));
    const decoded = promise.then((text) => decode(JSON.parse(text), bridge.host));
    resolve(json);
    bridge.release(n);

    expect(await decoded).toBeInstanceOf(FakeImage);
    await macrotask();
    expect(() => bridge.host(n)).toThrow(`[engine-bridge] host value ${n} was released`);
  });

  test('a release before the resolve still lets the decode read the image', async () => {
    // The other order `runtime::to_js` produces: `render_pages` and an
    // already-evicted handle drop the last Rust clone (`release`) just
    // before `resolver.settle` resolves the promise that carried it.
    const bridge = createEngineBridge(imageEngine());
    const { json, n } = await renderOne(bridge);
    let resolve!: (text: string) => void;
    const promise = new Promise<string>((done) => (resolve = done));
    const decoded = promise.then((text) => decode(JSON.parse(text), bridge.host));
    bridge.release(n);
    resolve(json);

    expect(await decoded).toBeInstanceOf(FakeImage);
    await macrotask();
    expect(() => bridge.host(n)).toThrow(`[engine-bridge] host value ${n} was released`);
  });

  test('releases every image released in one turn with one timeout', async () => {
    const bridge = createEngineBridge(imageEngine());
    const first = await renderOne(bridge);
    const second = await renderOne(bridge);
    const timeouts = vi.spyOn(globalThis, 'setTimeout');
    bridge.release(first.n);
    bridge.release(second.n);
    expect(timeouts).toHaveBeenCalledTimes(1);
    timeouts.mockRestore();
    await macrotask();
    expect(bridge.counts().hosts).toBe(0);
  });

  test('a second release throws, whether the first is pending or done', async () => {
    const bridge = createEngineBridge(imageEngine());
    const { n } = await renderOne(bridge);
    bridge.release(n);
    expect(() => bridge.release(n)).toThrow(`[engine-bridge] host value ${n} was already released`);
    await macrotask();
    expect(() => bridge.release(n)).toThrow(`[engine-bridge] host value ${n} was already released`);
    expect(errors).toHaveBeenCalledTimes(2);
  });

  test("releasing an id this bridge does not hold throws, and leaves the owner's alone", async () => {
    const owner = createEngineBridge(imageEngine());
    const other = createEngineBridge(imageEngine());
    const { n } = await renderOne(owner);
    expect(() => other.release(n)).toThrow(`[engine-bridge] no host value ${n}`);
    expect(() => other.release(999999)).toThrow('[engine-bridge] no host value 999999');
    await macrotask();
    expect(owner.host(n)).toBeInstanceOf(FakeImage);
  });

  test('a call that settles after dispose does not store its result', async () => {
    let settle!: (image: FakeImage) => void;
    const doc = {
      id: 'doc',
      page: () => ({ render: { image: () => new Promise<FakeImage>((done) => (settle = done)) } }),
    };
    const engine = { open: async () => doc } as unknown as Engine;
    const bridge = createEngineBridge(engine);
    await bridge.open(JSON.stringify('input'));
    const call = bridge.call('doc', 'page.render.image', 1, '[]');
    const sharedBefore = bridge.counts().allHosts;

    bridge.dispose();
    settle(new FakeImage());
    const json = await call;
    const answer = JSON.parse(json) as { $host: number; source: { bytes: { $hostBytes: number } } };

    // Encoding the late result still answers a `$host`/`$hostBytes` id (the
    // caller's decode is untouched), but neither id was stored: this
    // bridge's own count stays at zero and the shared table did not grow.
    expect(bridge.counts()).toMatchObject({ hosts: 0, allHosts: sharedBefore, documents: 0 });
    expect(() => bridge.host(answer.$host)).toThrow(
      `[engine-bridge] host value ${answer.$host} was released`,
    );
    expect(() => bridge.hostBytes(answer.source.bytes.$hostBytes)).toThrow(
      `[engine-bridge] host value ${answer.source.bytes.$hostBytes} was released`,
    );
  });

  test('releasing an id from before dispose is a harmless no-op afterwards', async () => {
    const bridge = createEngineBridge(imageEngine());
    const { n } = await renderOne(bridge);
    bridge.dispose();
    expect(() => bridge.release(n)).not.toThrow();
  });

  test('releaseHost releases through the owning bridge, whichever that is', async () => {
    // What `epdf_kernel::bytes::release_host` reaches, with no bridge in
    // hand: the image and its bytes both go, on the next macrotask.
    const owner = createEngineBridge(imageEngine());
    const other = createEngineBridge(imageEngine());
    const { n, m } = await renderOne(owner);
    releaseHost(n);
    expect(owner.host(n)).toBeInstanceOf(FakeImage);
    await macrotask();
    expect(owner.counts().hosts).toBe(0);
    expect(other.counts().hosts).toBe(0);
    expect(() => hostBytes(m)).toThrow(`[engine-bridge] host bytes ${m} was released`);
  });

  test('a second releaseHost throws loudly, whether the first is pending or done', async () => {
    const bridge = createEngineBridge(imageEngine());
    const { n } = await renderOne(bridge);
    releaseHost(n);
    expect(() => releaseHost(n)).toThrow(`[engine-bridge] host value ${n} was already released`);
    await macrotask();
    expect(() => releaseHost(n)).toThrow(`[engine-bridge] host value ${n} was already released`);
    expect(errors).toHaveBeenCalledTimes(2);
  });

  test('releaseHost throws for an id nothing ever held', () => {
    expect(() => releaseHost(999999)).toThrow('[engine-bridge] no host value 999999');
  });
});

describe('encodeInput', () => {
  test('reads a Blob to $bytes and keeps the rest canonical', async () => {
    const blob = new Blob([new Uint8Array([1, 2, 3])]);
    const json = await encodeInput({
      kind: 'bytes',
      id: 'x',
      bytes: blob,
      password: undefined,
      n: NaN,
    });
    expect(JSON.parse(json)).toEqual({
      kind: 'bytes',
      id: 'x',
      bytes: { $bytes: 'AQID' },
      n: { $number: 'NaN' },
    });
  });
});

describe('errorSpec', () => {
  test('an error that refers to itself still crosses as itself', () => {
    const hold = { hold: () => 7 };
    const error = Object.assign(new Error('bad xref'), { code: 'Format' }) as Error & {
      self?: unknown;
    };
    error.self = error;
    const once = { name: 'Error', message: 'bad xref', code: 'Format' };
    // How the bridge encodes an engine rejection (`reason`): the member
    // that leads back is encoded once, and left out the second time.
    expect(errorSpec(error, (v) => canonical(v, hold))).toEqual({
      $error: { ...once, self: { $error: once } },
    });
    expect(canonical(error, hold)).toEqual({ $error: once });
    expect(canonical({ error }, hold)).toEqual({ error: { $error: once } });
  });
});

describe('canonicalText', () => {
  const hooks = { hold: () => 7 };
  const viaCanonical = (value: unknown) => JSON.stringify(canonical(value, { hold: () => 7 }));

  test('writes what canonical writes, plain data or not', () => {
    const sparse: unknown[] = [1];
    sparse[2] = 3;
    const values: unknown[] = [
      null,
      true,
      'text',
      0,
      -0,
      1.5,
      [],
      {},
      { runs: [{ rect: { x: 1, y: 2.5, width: 3, height: 4 }, text: 'a', glyphs: [] }] },
      { kept: 1, dropped: undefined },
      Object.assign(Object.create(null), { a: 1 }),
      undefined,
      NaN,
      Infinity,
      [1, undefined],
      sparse,
      { n: NaN },
      { bytes: new Uint8Array([1, 2, 3]) },
      { when: new Date(5) },
      { method() {} },
      new Map([[1, 2]]),
      { big: 1n },
    ];
    for (const value of values) expect(canonicalText(value, hooks)).toBe(viaCanonical(value));
  });

  test('a cyclic value still throws', () => {
    const cyclic: Record<string, unknown> = {};
    cyclic.self = cyclic;
    expect(() => canonicalText(cyclic, hooks)).toThrow(TypeError);
  });
  test('plain data never reaches a hook, so any caller can skip canonical', () => {
    // The hooks the kernel shim encodes a call's arguments with: each one
    // marks what it touched, so a value that reaches one cannot pass.
    const reached = { $reached: true };
    const shim = {
      hold: () => 7,
      fn: () => reached,
      signal: () => {},
      blob: () => reached,
      bytes: () => reached,
    };
    const plain: unknown[] = [
      [],
      [1, 'a', null, true],
      [{ page: 3, rect: { x: 1, y: -0, width: 2, height: 3 } }],
      [{ kept: 1, dropped: undefined }],
    ];
    for (const value of plain) {
      expect(isPlainJson(value)).toBe(true);
      expect(JSON.stringify(value)).toBe(JSON.stringify(canonical(value, shim)));
    }
    const notPlain: unknown[] = [
      [() => {}],
      [{ onChange() {} }],
      [new AbortController().signal],
      [new Blob(['x'])],
      [new Uint8Array([1])],
      [undefined],
      [NaN],
      [new Date(5)],
    ];
    for (const value of notPlain) expect(isPlainJson(value)).toBe(false);
  });
});
