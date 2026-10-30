import { readFile } from 'node:fs/promises';
import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { createLocalEngine } from '@embedpdf/engine';
import type { Kernel } from '@embedpdf/core';
import { AnnotationToken } from '@embedpdf/plugin-annotation';
import { SelectionToken } from '@embedpdf/plugin-selection/contract/host';
import { StageToken } from '@embedpdf/plugin-stage';

import { WasmKernel, set_host_bytes_source, set_host_release } from '../wasm-node/epdf_web.js';
import { hostBytes, releaseHost } from '../src/engine-bridge';
import {
  annotationPlugin,
  createKernel,
  interactionPlugin,
  selectionPlugin,
  setWasmKernel,
  stagePlugin,
} from '../src';

const kernels: Kernel[] = [];
let bytes: Uint8Array;

beforeAll(async () => {
  setWasmKernel(WasmKernel);
  set_host_bytes_source(hostBytes);
  set_host_release(releaseHost);
  bytes = new Uint8Array(
    await readFile(new URL('../../engine/main/test/fixtures/hello_world.pdf', import.meta.url)),
  );
});

afterEach(async () => {
  for (const kernel of kernels.splice(0)) await kernel.destroy();
});

async function openFixture() {
  const kernel = createKernel({
    engine: createLocalEngine({ runtime: { prefer: 'wasm' } }),
    plugins: [stagePlugin(), interactionPlugin(), selectionPlugin(), annotationPlugin()],
  });
  kernels.push(kernel);
  await kernel.start();
  await kernel.documents.open({ kind: 'bytes', id: 'fixture', bytes }, { scope: ['*'] });
  const stage = kernel.capability(StageToken, 'fixture');
  stage.setViewportSize({ width: 800, height: 600 });
  const page = stage.listVisiblePages()[0].ref;
  const selection = kernel.capability(SelectionToken, 'fixture');
  await selection.ensureLoaded(page);
  selection.select({ page, start: 0, count: 5 });
  expect(await selection.readText()).toBe('Hello');
  return { kernel, page, selection, annotation: kernel.capability(AnnotationToken, 'fixture') };
}

test('creating a square emits its confirmed record and leaves text reads working', async () => {
  const { annotation, page, selection } = await openFixture();
  const created = vi.fn();
  const off = annotation.onCreated(created);
  try {
    const { annotation: record } = await annotation.create(page, {
      subtype: 'square',
      rect: { x: 10, y: 10, width: 50, height: 50 },
    });
    expect(record.subtype).toBe('square');
    expect(annotation.list().map((item) => item.ref)).toContainEqual(record.ref);
    expect(created).toHaveBeenCalledTimes(1);
    expect(created.mock.calls[0][0].annotation.ref).toEqual(record.ref);
    expect(await selection.readText()).toBe('Hello');
  } finally {
    off();
  }
}, 5000);

test('creating a highlight from selected text keeps the runtime responsive', async () => {
  const { annotation, selection } = await openFixture();
  const created = vi.fn();
  const off = annotation.onCreated(created);
  try {
    const { annotations } = await annotation.createFromSelection('highlight', { clear: false });
    expect(annotations).toHaveLength(1);
    expect(annotations[0].subtype).toBe('highlight');
    expect(annotation.list().map((item) => item.ref)).toContainEqual(annotations[0].ref);
    expect(created).toHaveBeenCalledTimes(1);
    expect(created.mock.calls[0][0].annotation.ref).toEqual(annotations[0].ref);
    expect(await selection.readText()).toBe('Hello');
  } finally {
    off();
  }
}, 5000);

test.each([0, 1])(
  'destroying viewer %i leaves the other able to edit and read text',
  async (index) => {
    const viewers = [await openFixture(), await openFixture()];
    await viewers[index].kernel.destroy();
    const { annotation, page, selection } = viewers[1 - index];
    const { annotation: record } = await annotation.create(page, {
      subtype: 'square',
      rect: { x: 10, y: 10, width: 50, height: 50 },
    });
    expect(annotation.list().map((item) => item.ref)).toContainEqual(record.ref);
    expect(await selection.readText()).toBe('Hello');
  },
  5000,
);
