import { test, expect } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import type {} from '../../../examples/viewer-react/src/e2e';

const requestPath = process.env.EPDF_PARITY_REQUEST;
if (requestPath) {
  test('managed parity snapshots from the packed real viewer', async ({ page, context }, info) => {
    const root = process.env.EPDF_PARITY_ROOT!;
    const output = process.env.EPDF_PARITY_VIEWER_OUTPUT!;
    const request = JSON.parse(fs.readFileSync(requestPath, 'utf8'));
    const errors: string[] = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await context.route('**/*', (route) => {
      const url = new URL(route.request().url());
      return url.origin === 'http://127.0.0.1:5241' || ['data:', 'blob:'].includes(url.protocol)
        ? route.continue()
        : route.abort('blockedbyclient');
    });
    await page.route('**/baseline.pdf', (route) =>
      route.fulfill({ path: path.resolve(root, request.pdf), contentType: 'application/pdf' }),
    );
    for (const [index, font] of request.fonts.entries()) {
      await page.route(`**/parity-fonts/${index}.ttf`, (route) =>
        route.fulfill({ path: path.resolve(root, font.path), contentType: 'font/ttf' }),
      );
    }
    await page.addInitScript(
      ({ fonts }) => {
        const fontConfiguration = { fonts: [], fallbackFonts: [] } as {
          fonts: { key: string; url: string }[];
          fallbackFonts: { key: string; url: string }[];
        };
        for (const [index, font] of fonts.entries()) {
          const spec = {
            key: font.key,
            familyName: font.familyName,
            weight: font.weight,
            italic: font.italic,
            url: `/parity-fonts/${index}.ttf`,
          };
          (font.fallback ? fontConfiguration.fallbackFonts : fontConfiguration.fonts).push(spec);
        }
        window.parityFontConfiguration = fontConfiguration;
        (window as unknown as { parityObservation: unknown }).parityObservation = {
          init: [],
          replies: [],
          pending: [],
        };
        const observed = new WeakSet<Worker>();
        const send = Worker.prototype.postMessage;
        Worker.prototype.postMessage = function (message, transfer = []) {
          const observation = (
            window as unknown as {
              parityObservation: {
                init: { rustRouting: unknown; wasmUrl: string | null; wasmSha256?: string }[];
                replies: string[];
                pending: Promise<void>[];
              };
            }
          ).parityObservation;
          if (!observed.has(this)) {
            observed.add(this);
            this.addEventListener('message', ({ data }) => {
              const body = data.json ?? data;
              if (body.kind === 'resolve' && body.result?.tag)
                observation.replies.push(body.result.tag);
            });
          }
          const body = message.json ?? message;
          if (body.kind === 'init') {
            const entry = {
              rustRouting: body.rustRouting ?? null,
              wasmUrl: body.wasmUrl?.startsWith('data:')
                ? 'data: (hashed)'
                : (body.wasmUrl ?? null),
            } as { rustRouting: unknown; wasmUrl: string | null; wasmSha256?: string };
            observation.init.push(entry);
            const bytes =
              body.wasmBinary instanceof ArrayBuffer
                ? Promise.resolve(body.wasmBinary.slice(0))
                : fetch(body.wasmUrl).then((response) => {
                    if (!response.ok) throw new Error('Cannot hash actual worker WASM source');
                    return response.arrayBuffer();
                  });
            observation.pending.push(
              bytes
                .then((buffer) => crypto.subtle.digest('SHA-256', buffer))
                .then((hash) => {
                  entry.wasmSha256 = Array.from(new Uint8Array(hash), (n) =>
                    n.toString(16).padStart(2, '0'),
                  ).join('');
                }),
            );
          }
          return Reflect.apply(send, this, [message, transfer]);
        };
      },
      { fonts: request.fonts },
    );
    await page.goto('/e2e.html?backend=typescript&parity=1');
    await expect
      .poll(() => page.evaluate(() => window.viewerBaseline?.snapshot().documents))
      .toEqual(['ready']);
    const raw = await page.evaluate(
      (probes) => window.viewerBaseline.paritySnapshot(probes),
      request.probes,
    );
    const observation = await page.evaluate(async () => {
      const live = (
        window as unknown as {
          parityObservation: {
            init: { rustRouting: unknown; wasmUrl: string | null; wasmSha256?: string }[];
            replies: string[];
            pending: Promise<void>[];
          };
        }
      ).parityObservation;
      await Promise.all(live.pending);
      return { init: live.init, replies: live.replies };
    });
    expect(observation.init).toHaveLength(1);
    expect(observation.init[0].rustRouting).toEqual({ rustReads: false, rustSession: false });
    expect(observation.init[0].wasmSha256).toBe(request.runtimeWasmSha256);
    expect(observation.replies).toContain('pages.list');
    expect(observation.replies).toContain('pages.geometry');
    expect(errors).toEqual([]);
    fs.writeFileSync(output, raw);
    fs.writeFileSync(
      output + '.backend.json',
      JSON.stringify(
        {
          actualBackend: 'typescript',
          evidence: observation,
          project: info.project.name,
          browser: await page.context().browser()?.version(),
          flags: { rustReads: false, rustSession: false },
          delivery: 'packed-production',
        },
        null,
        2,
      ) + '\n',
    );
  });
}
