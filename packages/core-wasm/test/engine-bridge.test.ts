import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, beforeAll, describe, expect, test } from 'vitest';
import { createLocalEngine } from '@embedpdf/engine';
import type { Engine } from '@embedpdf/engine-core/runtime';

import { createEngineBridge, encodeInput, type EngineBridge } from '../src/engine-bridge';

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
  });
  afterAll(async () => {
    await bridge.destroy();
  });

  test('open answers the handle id', () => {
    expect(docId).toBe('fixture');
  });

  test('pages.list answers the fixture page count as canonical JSON', async () => {
    const list = JSON.parse(await bridge.call(docId, 'pages.list', null, '[]'));
    expect(list.pageCount).toBe(2);
    expect(list.pages).toHaveLength(2);
    firstPon = list.pages[0].ref.pageObjectNumber;
    expect(typeof firstPon).toBe('number');
  });

  test('a page render answers its data with a $host key for the live handle', async () => {
    const answer = JSON.parse(
      await bridge.call(
        docId,
        'page.render.image',
        firstPon,
        JSON.stringify([{ width: 64, format: 'bmp' }]),
      ),
    );
    expect(answer.format).toBe('bmp');
    expect(answer.source.kind).toBe('bytes');
    expect(typeof answer.source.bytes.$bytes).toBe('string');
    expect(typeof answer.$host).toBe('number');
    const live = bridge.host(answer.$host) as { objectUrl?: unknown };
    expect(typeof live.objectUrl).toBe('function');
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

  test('close forgets the document', async () => {
    expect(JSON.parse(await bridge.call(docId, 'close', null, '[]'))).toEqual({ $undefined: true });
    expect(bridge.has(docId, 'render')).toBe(false);
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
