import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, beforeAll, describe, expect, test } from 'vitest';
import { createLocalEngine } from '@embedpdf/engine';
import type { Kernel } from '@embedpdf/core';
import { InteractionToken } from '@embedpdf/plugin-interaction';
import { StageToken } from '@embedpdf/plugin-stage';
import { CommandsToken } from '@embedpdf/plugin-commands';

import { WasmKernel } from '../wasm-node/epdf_web.js';
import {
  commandsPlugin,
  createKernel,
  interactionPlugin,
  renderPlugin,
  setWasmKernel,
  stagePlugin,
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
});

/** Start `plugins` over the local engine and open the fixture. */
async function openFixture(plugins: Parameters<typeof createKernel>[0]['plugins']) {
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
  k.documents.openAll([{ source: { kind: 'bytes', id: 'fixture', bytes }, scope: ['*'] }]);
  await ready;
  return k;
}

describe('the Kernel shim over the Rust kernel', () => {
  test('a getter answers the same object until the store changes', async () => {
    const k = await openFixture([stagePlugin(), interactionPlugin(), renderPlugin()]);
    const stage = k.capability(StageToken);
    expect(k.capability(StageToken, 'fixture')).toBe(stage);
    stage.setViewportSize({ width: 800, height: 600 });

    const visible = stage.listVisiblePages();
    expect(visible.length).toBeGreaterThan(0);
    expect(stage.listVisiblePages()).toBe(visible);
    expect(typeof visible[0].transform.toPixels).toBe('function');
    const { viewScale } = visible[0].transform;
    expect(visible[0].transform.toPixels({ x: 10, y: 20 })).toEqual({
      x: 10 * viewScale,
      y: 20 * viewScale,
    });

    stage.panBy(0, 100);
    expect(stage.listVisiblePages()).not.toBe(visible);
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

  test('a JS plugin with its own reducer is refused', () => {
    const engine = createLocalEngine({ runtime: { prefer: 'wasm' } });
    const plugin = { id: 'mine', reduce: (state: unknown) => state };
    expect(() => createKernel({ engine, plugins: [plugin] })).toThrow(
      '[core-wasm] JS plugin "mine" has reduce; only init-only JS plugins run in the eval build',
    );
  });
});
