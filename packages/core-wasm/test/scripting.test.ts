import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, expect, test, vi } from 'vitest';
import { createLocalEngine } from '@embedpdf/engine';
import type { Kernel } from '@embedpdf/core';
import { ActionsToken } from '@embedpdf/plugin-actions/contract/host';

import { WasmKernel } from '../wasm-node/epdf_web.js';
import { actionsPlugin, createKernel, formPlugin, interactionPlugin, setWasmKernel } from '../src';

const here = dirname(fileURLToPath(import.meta.url));
/** Its first page, object 3, has the page-open action `app.alert('opened')`
 *  (`generate.mjs`, `pageActions`). */
const FIXTURE = resolve(
  here,
  '..',
  '..',
  '..',
  'tooling',
  'engine-conformance',
  'fixtures',
  'page_actions.pdf',
);

let kernel: Kernel | undefined;

afterEach(async () => {
  await kernel?.destroy();
  kernel = undefined;
});

test('a page open action runs through the QuickJS sandbox by default', async () => {
  setWasmKernel(WasmKernel);
  const bytes = new Uint8Array(await readFile(FIXTURE));
  const engine = createLocalEngine({ runtime: { prefer: 'wasm' } });
  const k = createKernel({
    engine,
    plugins: [interactionPlugin(), actionsPlugin({ javascript: { enabled: true } }), formPlugin()],
  });
  kernel = k;
  await k.start();
  const ready = new Promise<string>((done) => {
    const off = k.documents.onOpened((event) => {
      off();
      done(event.documentId);
    });
  });
  k.documents.openAll([{ source: { kind: 'bytes', id: 'fixture', bytes }, scope: ['*'] }]);
  const doc = await ready;

  const alerts: string[] = [];
  const actions = k.capability(ActionsToken, doc);
  actions.setUiAdapter({
    openUri: () => {},
    print: () => {},
    alert: (message) => void alerts.push(message),
  });

  // What stage reports once the page is on screen.
  const page = { kind: 'objectNumber', pageObjectNumber: 3 } as const;
  actions.reportPageState({
    currentPage: page,
    visiblePages: [page],
    placed: true,
    cause: 'programmatic',
  });

  await vi.waitFor(() => expect(alerts).toContain('opened'), { timeout: 5_000 });
}, 15_000);
