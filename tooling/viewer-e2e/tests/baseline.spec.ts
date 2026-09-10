import { test, expect, type Page } from '@playwright/test';
import { installPerformanceProbe, distribution } from '../performance';
import { fileURLToPath } from 'node:url';
import type {} from '../../../examples/viewer-react/src/e2e';

const fixture = fileURLToPath(new URL('../fixtures/baseline.pdf', import.meta.url));
const performanceEnabled = process.env.VIEWER_E2E_PERF === '1';
const flowStarts = new WeakMap<Page, number>();
const errors = new WeakMap<Page, string[]>();
const snapshot = (page: Page) => page.evaluate(() => window.viewerBaseline.snapshot());
// The base image is a direct child of the page content surface. Tiles live
// one level deeper. No generated class names or framework internals.
const bases = (page: Page) =>
  page.locator('embedpdf-viewer img[draggable="false"]').filter({
    visible: true,
  });
const stage = (page: Page) =>
  page.locator('[data-embedpdf-scrollbar][data-axis="y"]').locator('..');

declare global {
  interface Window {
    renderProbe: { requests: string[]; pending: number[] };
  }
}

async function painted(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(() => {
        const root = document.querySelector('embedpdf-viewer')?.shadowRoot;
        const images = [
          ...(root?.querySelectorAll('img[draggable="false"]') ?? []),
        ] as HTMLImageElement[];
        return images.length > 0 && images.every((img) => img.complete && img.naturalWidth > 0);
      }),
    )
    .toBe(true);
  await expect.poll(() => page.evaluate(() => window.renderProbe.pending.length)).toBe(0);
}

// Prove visible tile coverage, including tiles that never entered the DOM.
// Subdivide at each tile edge; every cell of the visible page must be covered.
async function tilesCoverViewport(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(() => {
        const root = document.querySelector('embedpdf-viewer')!.shadowRoot!;
        const viewport = root
          .querySelector('[data-embedpdf-scrollbar][data-axis="y"]')!
          .parentElement!.getBoundingClientRect();
        const images = [...root.querySelectorAll<HTMLImageElement>('img[draggable="false"]')];
        const baseImages = images.filter((img) => img.style.width === '100%');
        let visible = 0;
        for (const base of baseImages) {
          const b = base.getBoundingClientRect();
          const left = Math.max(b.left, viewport.left),
            right = Math.min(b.right, viewport.right);
          const top = Math.max(b.top, viewport.top),
            bottom = Math.min(b.bottom, viewport.bottom);
          if (right <= left || bottom <= top) continue;
          visible++;
          const tiles = [...base.parentElement!.querySelectorAll<HTMLImageElement>('div > img')]
            .filter(
              (img) =>
                img !== base &&
                img.complete &&
                img.naturalWidth > 0 &&
                getComputedStyle(img).visibility !== 'hidden',
            )
            .map((img) => img.getBoundingClientRect());
          const xs = [
            ...new Set([
              left,
              right,
              ...tiles.flatMap((t) => [t.left, t.right]).filter((x) => x > left && x < right),
            ]),
          ].sort((a, b) => a - b);
          const ys = [
            ...new Set([
              top,
              bottom,
              ...tiles.flatMap((t) => [t.top, t.bottom]).filter((y) => y > top && y < bottom),
            ]),
          ].sort((a, b) => a - b);
          for (let x = 1; x < xs.length; x++)
            for (let y = 1; y < ys.length; y++) {
              const cx = (xs[x - 1] + xs[x]) / 2,
                cy = (ys[y - 1] + ys[y]) / 2;
              if (
                !tiles.some(
                  (t) =>
                    cx >= t.left - 0.5 &&
                    cx <= t.right + 0.5 &&
                    cy >= t.top - 0.5 &&
                    cy <= t.bottom + 0.5,
                )
              )
                return false;
            }
        }
        return visible > 0;
      }),
    )
    .toBe(true);
}

async function zoomTo(page: Page, percent: number) {
  await page.getByRole('textbox', { name: 'Set zoom' }).fill(String(percent));
  await page.getByRole('textbox', { name: 'Set zoom' }).press('Enter');
  await expect.poll(async () => (await snapshot(page)).zoom).toBe(percent / 100);
  await painted(page);
}

async function dragText(page: Page, from: { page: number; x: number; y: number }, to: typeof from) {
  const box = await stage(page).boundingBox();
  expect(box).not.toBeNull();
  const state = await snapshot(page);
  const point = (p: typeof from) => {
    const rect = state.pages.find((r) => r.index === p.page)!;
    expect(rect).toBeDefined();
    return {
      x: box!.x + rect.x + (p.x * rect.width) / 600,
      y: box!.y + rect.y + (p.y * rect.height) / 400,
    };
  };
  const a = point(from),
    b = point(to);
  await page.mouse.move(a.x, a.y);
  await page.mouse.down();
  await page.mouse.move(b.x, b.y, { steps: 20 });
  await page.mouse.up();
}

// Capture the real copy event payload. This avoids OS clipboard permissions
// (which differ across browsers) without replacing the viewer's copy handler.
async function copiedText(page: Page) {
  return page.evaluate(() => {
    const data = new DataTransfer();
    const event = new ClipboardEvent('copy', {
      clipboardData: data,
      bubbles: true,
      cancelable: true,
    });
    document.dispatchEvent(event);
    return { handled: event.defaultPrevented, text: event.clipboardData?.getData('text/plain') };
  });
}

test.beforeEach(async ({ page, context }, info) => {
  if (performanceEnabled) await page.addInitScript(installPerformanceProbe);
  errors.set(page, []);
  page.on('pageerror', (error) => errors.get(page)!.push(error.message));
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    return url.origin.startsWith('http://127.0.0.1:') || url.protocol === 'data:'
      ? route.continue()
      : route.abort('blockedbyclient');
  });
  await page.route('**/baseline.pdf', (route) =>
    route.fulfill({ path: fixture, contentType: 'application/pdf' }),
  );
  await page.addInitScript(() => {
    window.renderProbe = { requests: [], pending: [] };
    const send = Worker.prototype.postMessage;
    const observed = new WeakSet<Worker>();
    const timings = new WeakMap<Worker, Map<number, { start: number; kind: string }>>();
    Worker.prototype.postMessage = function (
      message,
      transfer: Transferable[] | StructuredSerializeOptions = [],
    ) {
      if (!observed.has(this)) {
        observed.add(this);
        timings.set(this, new Map());
        this.addEventListener('message', ({ data }) => {
          const timing = timings.get(this)!.get(data.jobId);
          if (timing && window.viewerPerformance) {
            window.viewerPerformance.renders.push({
              kind: timing.kind,
              durationMs: performance.now() - timing.start,
              outcome: data.kind,
            });
            timings.get(this)!.delete(data.jobId);
          }
          window.renderProbe.pending = window.renderProbe.pending.filter((id) => id !== data.jobId);
        });
      }
      if (message.kind === 'pages.render' || message.kind === 'pages.renderEncoded') {
        window.renderProbe.requests.push(
          JSON.stringify({
            kind: message.kind,
            pon: message.pageObjectNumber,
            options: message.options,
          }),
        );
        window.renderProbe.pending.push(message.jobId);
        if (window.viewerPerformance)
          timings.get(this)!.set(message.jobId, {
            start: performance.now(),
            kind: message.kind,
          });
      }
      return Reflect.apply(send, this, [message, transfer]);
    };
  });
  await page.goto(`/e2e.html?backend=${info.project.metadata.backend}`);
  await expect(page.getByText('Page 1 of 12', { exact: true })).toBeVisible();
  await page.locator('embedpdf-viewer').evaluate((el) => {
    const style = document.createElement('style');
    style.textContent =
      '*, *::before, *::after { animation: none !important; transition: none !important; scroll-behavior: auto !important; }';
    el.shadowRoot!.append(style);
  });
  await painted(page);
  if (performanceEnabled) {
    await page.waitForFunction(() => window.viewerPerformance?.firstPageFrameMs != null);
    flowStarts.set(page, performance.now());
  }
});

test.afterEach(async ({ page, browser }, info) => {
  const flowEnd = performance.now();
  if (performanceEnabled && !page.isClosed()) {
    const probe = await page.evaluate(() => window.viewerPerformance);
    if (probe) {
      const metrics: Record<string, number> = {};
      if (probe.firstPageFrameMs !== null)
        metrics.navigationToFirstPageFrameMs = probe.firstPageFrameMs;
      const start = flowStarts.get(page);
      if (start !== undefined) metrics.flowWallMs = flowEnd - start;
      const renders = probe.renders.filter((r) => r.outcome === 'resolve').map((r) => r.durationMs);
      if (renders.length) {
        metrics.renderRoundTripMedianMs = distribution(renders).median;
        metrics.renderRoundTripP95Ms = distribution(renders).p95;
      }
      await info.attach('viewer-performance', {
        contentType: 'application/json',
        body: Buffer.from(
          JSON.stringify({
            project: info.project.name,
            flow: info.title,
            repeat: info.repeatEachIndex,
            retry: info.retry,
            browserVersion: browser.version(),
            metrics,
            renders: probe.renders,
          }),
        ),
      });
    }
  }
  expect(errors.get(page), 'uncaught browser errors').toEqual([]);
});

test('F1 opens the fixture, reports its page count and paints known pixels', async ({ page }) => {
  expect((await snapshot(page)).backend).toBe('typescript');
  expect((await snapshot(page)).documents).toEqual(['ready']);
  // Pixel content, not just successful image decode: a blank/white raster
  // must fail. The fixed red square's interior is independent of font AA.
  await expect
    .poll(() =>
      bases(page)
        .first()
        .evaluate((node) => {
          const img = node as HTMLImageElement;
          const canvas = document.createElement('canvas');
          canvas.width = img.naturalWidth;
          canvas.height = img.naturalHeight;
          const ctx = canvas.getContext('2d')!;
          ctx.drawImage(img, 0, 0);
          return [
            ...ctx.getImageData(
              Math.round((canvas.width * 520) / 600),
              Math.round(canvas.height / 2),
              1,
              1,
            ).data,
          ];
        }),
    )
    .toEqual([255, 0, 0, 255]);
});

test('F2 scrolls to a late page, virtualizes and retains resolved tiles', async ({ page }) => {
  await zoomTo(page, 200); // exceed the 640px base budget: exercise actual tiles
  await stage(page).hover();
  await page.mouse.wheel(0, 9000);
  await expect.poll(async () => (await snapshot(page)).currentPage).toBeGreaterThanOrEqual(7);
  await painted(page);
  await tilesCoverViewport(page);
  expect((await snapshot(page)).pages.length).toBeLessThan(12);
  expect(await bases(page).count()).toBeGreaterThan(2);
  const before = await page.evaluate(() => window.renderProbe.requests.slice());
  expect(before.length).toBeGreaterThan(0);
  // A small pan keeps currently visible tiles held. They must not be fetched
  // again; newly exposed tiles are allowed to arrive.
  const position = (await snapshot(page)).pages[0].y;
  await page.mouse.wheel(0, 8);
  await expect.poll(async () => (await snapshot(page)).pages[0].y).not.toBe(position);
  await painted(page);
  await tilesCoverViewport(page);
  const after = await page.evaluate(() => window.renderProbe.requests.slice());
  expect(after.slice(before.length).filter((key) => before.includes(key))).toEqual([]);
});

test('F3 zooms through wheel gestures with consistent page geometry and resolved tiles', async ({
  page,
}) => {
  await zoomTo(page, 100);
  await stage(page).hover();
  await page.keyboard.down('Control');
  await page.mouse.wheel(0, -200);
  await page.keyboard.up('Control');
  await expect.poll(async () => (await snapshot(page)).zoom).toBeGreaterThan(1);
  await painted(page);
  await tilesCoverViewport(page);
  const enlarged = await snapshot(page);
  const imageBox = await bases(page).first().boundingBox();
  expect(Math.abs(imageBox!.width - enlarged.pages[0].width)).toBeLessThanOrEqual(1);
  // PDF points are 1/72 inch; browser CSS pixels are 1/96 inch.

  for (const rect of enlarged.pages) {
    expect(Math.abs(rect.width - 600 * (96 / 72) * enlarged.zoom!)).toBeLessThanOrEqual(1);
    expect(Math.abs(rect.height - 400 * (96 / 72) * enlarged.zoom!)).toBeLessThanOrEqual(1);
  }
  await page.keyboard.down('Control');
  await page.mouse.wheel(0, 200);
  await page.keyboard.up('Control');
  await expect.poll(async () => (await snapshot(page)).zoom).toBeLessThan(enlarged.zoom!);
  await painted(page);
  const reduced = await snapshot(page);
  expect(Math.abs(reduced.pages[0].width - 600 * (96 / 72) * reduced.zoom!)).toBeLessThanOrEqual(1);
});

test('F4 drag-selects and copies a line and a page boundary', async ({ page }) => {
  await zoomTo(page, 75);
  await dragText(page, { page: 0, x: 40, y: 54 }, { page: 0, x: 279, y: 54 });
  await expect
    .poll(() => page.evaluate(() => window.viewerBaseline.selectionText()))
    .toBe('Viewer baseline page 01');
  await expect
    .poll(() => copiedText(page))
    .toEqual({ handled: true, text: 'Viewer baseline page 01' });
  await expect(
    page.locator('embedpdf-viewer polygon[fill="rgba(33, 150, 243, 0.35)"]').first(),
  ).toBeInViewport();
  await dragText(page, { page: 0, x: 40, y: 344 }, { page: 1, x: 279, y: 54 });
  await expect
    .poll(() => copiedText(page))
    .toEqual({ handled: true, text: 'Boundary 01\nViewer baseline page 02' });
});

test('F5 searches and navigates every match in both directions', async ({ page }) => {
  await page.getByRole('button', { name: 'Search', exact: true }).click();
  await page.getByPlaceholder('Search document…').fill('needle');
  await expect.poll(async () => (await snapshot(page)).search?.count).toBe(12);
  await expect(page.getByText('12 results found', { exact: true })).toBeVisible();
  const indices = [
    ...Array.from({ length: 12 }, (_, i) => i),
    ...Array.from({ length: 11 }, (_, i) => 10 - i),
  ];
  for (const [step, index] of indices.entries()) {
    if (step > 0) await page.getByTitle(step <= 11 ? 'Next' : 'Previous', { exact: true }).click();
    await expect.poll(async () => (await snapshot(page)).search?.active).toBe(index);
    await expect(page.getByText(`Page ${index + 1} of 12`, { exact: true })).toBeVisible();
    // SearchLayer's active highlight; the sidebar snippet cannot satisfy it.
    await expect(
      page.locator('embedpdf-viewer div[style*="rgb(255, 150, 50)"]').first(),
    ).toBeInViewport();
    await painted(page);
  }
});
