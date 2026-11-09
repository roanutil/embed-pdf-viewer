/**
 * render's `handOverPictures` and `getPicturePainter` over the Rust kernel:
 * the painter crosses as the host's own value, so the page layers read back
 * the very object that was handed over (`useSelector` compares by
 * reference), and calling it never reaches Rust. And `reportPicture`, which
 * every page layer calls when it mounts: each call is a report of its own.
 */
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeAll, describe, expect, test } from 'vitest';
import { createLocalEngine } from '@embedpdf/engine';
import { isPluginError, type Kernel } from '@embedpdf/core';
import type { PicturePainter } from '@embedpdf/plugin-render';
import { RenderToken } from '@embedpdf/plugin-render/contract/host';

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

let bytes: Uint8Array;
let kernel: Kernel | undefined;

beforeAll(async () => {
  setWasmKernel(WasmKernel);
  set_host_bytes_source(hostBytes);
  set_host_release(releaseHost);
  bytes = new Uint8Array(await readFile(resolve(fixtures, 'two_page_sigfield.pdf')));
});
afterEach(async () => {
  await kernel?.destroy();
  kernel = undefined;
});

async function opened(): Promise<Kernel> {
  const engine = createLocalEngine({ runtime: { prefer: 'wasm' } });
  const k = createKernel({ engine, plugins: [stagePlugin(), interactionPlugin(), renderPlugin()] });
  kernel = k;
  await k.start();
  await k.documents.open({ kind: 'bytes', id: 'fixture', bytes });
  return k;
}

const painter = (views: string[]): PicturePainter<HTMLElement> => ({
  paints: (view) => views.includes(view),
  paint: () => ({ update() {}, destroy() {} }),
});

describe('another plugin painting the pictures', () => {
  test('getPicturePainter answers the painter handed over, by reference, until it is given back', async () => {
    const k = await opened();
    const render = k.capability(RenderToken);
    expect(render.getPicturePainter()).toBeNull();

    let notified = 0;
    const off = k.subscribe(() => {
      notified += 1;
    });
    const held = painter(['stage']);
    const giveBack = render.handOverPictures(held);
    expect(render.getPicturePainter()).toBe(held);
    expect(render.getPicturePainter()?.paints('stage')).toBe(true);
    expect(render.getPicturePainter()?.paints('thumbnails')).toBe(false);

    giveBack();
    expect(render.getPicturePainter()).toBeNull();
    // Giving back again does nothing, even once another painter holds them.
    const next = painter(['thumbnails']);
    render.handOverPictures(next);
    giveBack();
    expect(render.getPicturePainter()).toBe(next);
    await new Promise((done) => setTimeout(done, 40));
    expect(notified).toBeGreaterThan(0);
    off();
  });

  test('a second painter is refused conflict while the first holds them', async () => {
    const k = await opened();
    const render = k.capability(RenderToken);
    await new Promise((done) => setTimeout(done, 0));
    const held = kernelCounts(k).hosts;
    const first = painter(['stage']);
    const giveBack = render.handOverPictures(first);
    let refused: unknown;
    try {
      render.handOverPictures(painter(['thumbnails']));
    } catch (error) {
      refused = error;
    }
    expect(isPluginError(refused)).toBe(true);
    expect(refused).toMatchObject({
      code: 'conflict',
      message: 'another plugin paints the pictures already',
    });
    expect(render.getPicturePainter()).toBe(first);
    giveBack();
    expect(render.getPicturePainter()).toBeNull();
    // Neither painter is held on this side any more.
    await new Promise((done) => setTimeout(done, 0));
    expect(kernelCounts(k).hosts).toBe(held);
  });

  test('a reopened document starts with no painter', async () => {
    const k = await opened();
    k.capability(RenderToken).handOverPictures(painter(['stage']));
    await k.documents.close('fixture');
    await k.documents.open({ kind: 'bytes', id: 'fixture', bytes });
    expect(k.capability(RenderToken).getPicturePainter()).toBeNull();
  });

  test('each reportPicture call is a report of its own, and the page state follows them', async () => {
    const k = await opened();
    const render = k.capability(RenderToken);
    const page = k.documents.listPages('fixture')[0].ref;
    expect(render.getPictureState(page, 'stage')).toEqual({ status: 'idle', slow: false });

    // A layer mounts, unmounts and mounts again with the same arguments.
    const first = render.reportPicture('stage', page);
    expect(render.getPictureState(page, 'stage')).toEqual({ status: 'loading', slow: false });
    first.close();
    expect(render.getPictureState(page, 'stage')).toEqual({ status: 'idle', slow: false });
    const second = render.reportPicture('stage', page);
    expect(second).not.toBe(first);
    second.shown();
    expect(render.getPictureState(page, 'stage')).toEqual({ status: 'ready', slow: false });
    // The same object while it's the same state, as `useSelector` needs.
    expect(render.getPictureState(page, 'stage')).toBe(render.getPictureState(page, 'stage'));
    second.close();
    expect(render.getPictureState(page, 'stage')).toEqual({ status: 'idle', slow: false });
  });
});
