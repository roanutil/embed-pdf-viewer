import { test, expect, type Browser, type Page, type TestInfo } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import type {} from '../../../examples/viewer-react/src/e2e';

/**
 * F10 through F14: the write round trips.
 *
 * Each one makes a change, saves, and reopens the SAVED BYTES in a fresh
 * viewer. The fresh instance is the point. Reopening in the same one would
 * read the same in-memory model the mutation already updated, which proves
 * the UI reacted and says nothing about what reached the file — and what
 * reached the file is the only thing a user keeps.
 *
 * The mutations go through real controls wherever the viewer has one: a drag
 * with the rectangle tool, typing into a form field, the redaction tool and
 * its Apply button. Where it has none, the flow says so rather than reaching
 * around the UI.
 *
 * Two of the five cannot be written as the port order specifies them, and
 * both are recorded below with the active half that IS reachable, so the gap
 * is a failing assertion the day it closes rather than a comment nobody
 * reads.
 */
const fixture = fileURLToPath(new URL('../fixtures/features.pdf', import.meta.url));
const errors = new WeakMap<Page, string[]>();

/** The three pages of `features.pdf`, by the object numbers the file gives. */
const LINKS = 5;
const FORM = 7;

const snapshot = (page: Page) => page.evaluate(() => window.viewerBaseline.snapshot());
const stage = (page: Page) =>
  page.locator('[data-embedpdf-scrollbar][data-axis="y"]').locator('..');

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
}

/**
 * A point inside a page, from a PDF-space coordinate.
 *
 * The fixture's pages are 600x400 with the origin bottom-left and the viewer
 * lays them out top-down, so the y flips. Derived from the stage's own
 * reported geometry rather than from a zoom level, because the viewer picks
 * the fit and a test should not have to predict it.
 */
async function pointAt(page: Page, pageIndex: number, pdfX: number, pdfY: number) {
  const box = await stage(page).boundingBox();
  expect(box).not.toBeNull();
  const rect = (await snapshot(page)).pages.find((r) => r.index === pageIndex);
  expect(rect, `page ${pageIndex} is not visible`).toBeDefined();
  return {
    x: box!.x + rect!.x + (pdfX / 600) * rect!.width,
    y: box!.y + rect!.y + ((400 - pdfY) / 400) * rect!.height,
  };
}

/** Everything a page needs before a flow touches it. */
async function arrive(page: Page, info: TestInfo, doc: string) {
  errors.set(page, []);
  page.on('pageerror', (error) => errors.get(page)!.push(error.message));
  // Nothing leaves the machine. `blob:` is allowed alongside `data:`
  // because the viewer delivers a save as a blob URL, and blocking it makes
  // the Download entry hang rather than fail.
  await page.context().route('**/*', (route) => {
    const url = new URL(route.request().url());
    return url.origin.startsWith('http://127.0.0.1:') ||
      url.protocol === 'data:' ||
      url.protocol === 'blob:'
      ? route.continue()
      : route.abort('blockedbyclient');
  });
  await page.route('**/features.pdf', (route) =>
    route.fulfill({ path: fixture, contentType: 'application/pdf' }),
  );
  await page.goto(`/e2e.html?backend=${info.project.metadata.backend}&doc=${doc}`);
  await expect(page.getByText('Page 1 of 3', { exact: true })).toBeVisible();
  await page.locator('embedpdf-viewer').evaluate((el) => {
    const style = document.createElement('style');
    style.textContent =
      '*, *::before, *::after { animation: none !important; transition: none !important; scroll-behavior: auto !important; }';
    el.shadowRoot!.append(style);
  });
  await painted(page);
}

/**
 * Save through the viewer's own Download entry, and read the bytes back.
 *
 * Not the bridge's `documents.download`. The chrome's command wraps the save
 * in `runDocumentVerb('save', ...)`, which serializes it against the
 * document's other in-flight work; calling the capability directly skips
 * that, and a save fired straight after a delete then races it. The first
 * version of this file did exactly that and read a deleted annotation back
 * out of the file.
 *
 * It is also the path a user takes, which is what these flows are for.
 */
async function save(page: Page): Promise<number[]> {
  const started = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Menu' }).first().click();
  await page.getByRole('menuitem', { name: 'Download', exact: true }).click();
  const download = await started;
  const stream = await download.createReadStream();
  const chunks: Buffer[] = [];
  for await (const chunk of stream) chunks.push(chunk as Buffer);
  const bytes = [...Buffer.concat(chunks)];
  expect(bytes.length, 'the save produced no bytes').toBeGreaterThan(0);
  expect(String.fromCharCode(...bytes.slice(0, 5))).toBe('%PDF-');
  return bytes;
}

/**
 * Open saved bytes in a FRESH viewer, in its own browser context.
 *
 * A new context rather than a new tab: the two share nothing — no worker, no
 * engine, no cache — so what the second one reads it read out of the bytes.
 */
async function reopen(browser: Browser, info: TestInfo, bytes: number[]) {
  const context = await browser.newContext();
  const page = await context.newPage();
  errors.set(page, []);
  page.on('pageerror', (error) => errors.get(page)!.push(error.message));
  const body = Buffer.from(bytes);
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    return url.origin.startsWith('http://127.0.0.1:') ||
      url.protocol === 'data:' ||
      url.protocol === 'blob:'
      ? route.continue()
      : route.abort('blockedbyclient');
  });
  await page.route('**/roundtrip.pdf', (route) =>
    route.fulfill({ body, contentType: 'application/pdf' }),
  );
  await page.goto(`/e2e.html?backend=${info.project.metadata.backend}&doc=roundtrip`);
  await expect
    .poll(() => page.evaluate(() => window.viewerBaseline?.documentStatuses()[0]?.status ?? null))
    .toBe('ready');
  await painted(page);
  return { page, context };
}

/**
 * Pick the rectangle tool and drag one out on a page.
 *
 * The Shapes button TOGGLES its band, so opening it unconditionally closes
 * it again on the second call. Opened only when the tool it holds is not
 * already on screen.
 */
async function drawRectangle(
  page: Page,
  pageIndex: number,
  from: { x: number; y: number },
  to: { x: number; y: number },
) {
  const rectangle = page.getByRole('button', { name: 'Rectangle' });
  if ((await rectangle.count()) === 0) {
    await page.getByRole('button', { name: 'Shapes' }).click();
  }
  await rectangle.click();
  const a = await pointAt(page, pageIndex, from.x, from.y);
  const b = await pointAt(page, pageIndex, to.x, to.y);
  await page.mouse.move(a.x, a.y);
  await page.mouse.down();
  await page.mouse.move(b.x, b.y, { steps: 12 });
  await page.mouse.up();
}

/** The squares on a page: the drawn ones, with the fixture's links excluded. */
async function squares(page: Page, pon: number) {
  const items = await page.evaluate((p) => window.viewerBaseline.annotations(p), pon);
  return (items ?? []).filter((item) => item.subtype === 'square');
}

test.afterEach(async ({ page }) => {
  expect(errors.get(page), 'uncaught browser errors').toEqual([]);
});

test('F10 an annotation drawn through the UI survives a save and a reopen', async ({
  page,
  browser,
}, info) => {
  await arrive(page, info, 'features');
  expect(await squares(page, LINKS), 'the fixture starts with no square here').toHaveLength(0);

  await drawRectangle(page, 0, { x: 420, y: 330 }, { x: 560, y: 230 });
  await expect.poll(async () => (await squares(page, LINKS)).length).toBe(1);

  const bytes = await save(page);
  const { page: fresh, context } = await reopen(browser, info, bytes);
  try {
    // Read out of the saved file by an engine that never saw the drag.
    await expect.poll(async () => (await squares(fresh, LINKS)).length).toBe(1);
    const [survivor] = await squares(fresh, LINKS);
    const [drawn] = await squares(page, LINKS);
    // The same rectangle, not merely a rectangle. A writer that saved a
    // default-sized square would pass a count.
    expect(survivor.rect).toEqual(drawn.rect);
    expect(errors.get(fresh), 'uncaught browser errors in the fresh viewer').toEqual([]);
  } finally {
    await context.close();
  }
});

test('F10 deleting through the UI removes one annotation and keeps the other', async ({
  page,
  browser,
}, info) => {
  await arrive(page, info, 'features');

  // TWO squares, and only one deleted. One square deleted down to zero would
  // read exactly like a save that wrote nothing at all, since the fixture
  // starts with no squares either; a survivor is what tells the two apart.
  await drawRectangle(page, 0, { x: 420, y: 330 }, { x: 560, y: 230 });
  await expect.poll(async () => (await squares(page, LINKS)).length).toBe(1);
  const [keeper] = await squares(page, LINKS);

  await drawRectangle(page, 0, { x: 420, y: 210 }, { x: 560, y: 150 });
  await expect.poll(async () => (await squares(page, LINKS)).length).toBe(2);

  // The second one is the live selection, and the Delete button is the one
  // the viewer shows for it, which is what a user clicks.
  await expect
    .poll(() => page.evaluate(() => window.viewerBaseline.annotationSelection()?.length ?? 0))
    .toBe(1);
  await page.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect.poll(async () => (await squares(page, LINKS)).length).toBe(1);

  const bytes = await save(page);
  const { page: fresh, context } = await reopen(browser, info, bytes);
  try {
    await expect.poll(async () => (await squares(fresh, LINKS)).length).toBe(1);
    const [survivor] = await squares(fresh, LINKS);
    // The one that was kept, not the one that was deleted.
    expect(survivor.rect).toEqual(keeper.rect);
    // And the document is otherwise intact: the fixture's own annotations
    // are still there, so the delete took one thing and not the page.
    const links = await fresh.evaluate((p) => window.viewerBaseline.links(p), LINKS);
    expect(links).toHaveLength(2);
  } finally {
    await context.close();
  }
});

test('F11 a form value typed through the UI persists across a reopen', async ({
  page,
  browser,
}, info) => {
  await arrive(page, info, 'features');

  // The widget's own input, which the form layer renders over the page. The
  // fixture ships this field holding "Ada Lovelace".
  const field = page.locator('embedpdf-viewer').getByRole('textbox', { name: 'fullName' });
  await expect(field).toHaveValue('Ada Lovelace');
  await field.fill('Grace Hopper');
  // Committed on blur, the way a form fill actually ends.
  await field.blur();

  await expect
    .poll(async () => {
      const fields = await page.evaluate(() => window.viewerBaseline.formFields());
      return fields?.find((f) => f.name === 'fullName')?.value ?? null;
    })
    .toBe('Grace Hopper');

  const bytes = await save(page);
  const { page: fresh, context } = await reopen(browser, info, bytes);
  try {
    const fields = await fresh.evaluate(() => window.viewerBaseline.formFields());
    const byName = new Map((fields ?? []).map((f) => [f.name, f]));
    expect(byName.get('fullName')).toMatchObject({ value: 'Grace Hopper' });
    // The fields nobody touched are unchanged. A form write that rebuilt the
    // whole `/AcroForm` would pass on the field it wrote and lose these.
    expect(byName.get('subscribe')).toMatchObject({ checked: true });
    expect(byName.get('language')).toMatchObject({ selectedValues: ['fr'] });
  } finally {
    await context.close();
  }
});

test('F13 a redaction applied through the UI removes the text from the file', async ({
  page,
  browser,
}, info) => {
  await arrive(page, info, 'features');

  // The needle is there to start with, or the assertion at the end proves
  // nothing.
  await page.evaluate(() => window.viewerBaseline.search('CONFIDENTIAL'));
  await expect.poll(async () => (await snapshot(page)).search?.count ?? -1).toBe(1);

  // Marked with the redaction tool, over the line of text the fixture puts
  // clear of both link rects.
  await page.getByRole('button', { name: 'Redact' }).click();
  await page.getByRole('button', { name: 'Mark for Redaction' }).click();
  const from = await pointAt(page, 0, 40, 120);
  const to = await pointAt(page, 0, 340, 108);
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(to.x, to.y, { steps: 16 });
  await page.mouse.up();

  // Applied from the panel, and then CONFIRMED. The panel's button opens a
  // dialog rather than doing the work: applying a redaction cannot be
  // undone, so the viewer asks first, and a flow that stopped at the panel
  // button would read a document nothing had happened to.
  await page.getByRole('button', { name: 'Redactions' }).click();
  await page.getByRole('button', { name: 'Apply Redactions' }).click();
  const confirm = page.getByRole('button', { name: 'Apply', exact: true });
  await expect(confirm, 'the confirmation dialog did not open').toBeVisible();
  await confirm.click();
  // The marking goes when the content under it does.
  await expect
    .poll(async () => {
      const items = await page.evaluate((p) => window.viewerBaseline.annotations(p), LINKS);
      return (items ?? []).filter((item) => item.subtype === 'redact').length;
    })
    .toBe(0);

  const bytes = await save(page);
  const { page: fresh, context } = await reopen(browser, info, bytes);
  try {
    // Gone from the EXTRACTED text, which is what search reads. A redaction
    // that only painted a black box would leave this at one, and that is the
    // failure the whole feature exists to prevent.
    await fresh.evaluate(() => window.viewerBaseline.search('CONFIDENTIAL'));
    await expect.poll(async () => (await snapshot(fresh)).search?.status ?? '').toBe('complete');
    expect((await snapshot(fresh)).search?.count).toBe(0);

    // The rest of the page is still readable, so the apply removed a marking
    // rather than the content stream.
    await fresh.evaluate(() => window.viewerBaseline.search('Links page'));
    await expect.poll(async () => (await snapshot(fresh)).search?.count ?? -1).toBe(1);
  } finally {
    await context.close();
  }
});

test('F12 page identity survives a save and a reopen', async ({ page, browser }, info) => {
  // The half of F12 that is reachable, and the port order asks for it
  // separately: "verify stable page identity separately before closing".
  //
  // The other half is not written. It wants rotate, delete and reorder
  // driven through the UI, and this viewer has no page-assembly UI at all:
  // `rotate:clockwise` in the command table is `stage.rotateView`, a camera
  // move that writes nothing, and the sidebar's thumbnails carry no
  // controls, no hover affordances and no context menu. The `page-edit`
  // capability exists and is registered, but it is deliberately outside the
  // curated token list `@embedpdf/viewer` publishes, so reaching it from
  // here would mean widening a public API for a test.
  await arrive(page, info, 'features');

  const before = await page.evaluate(() => window.viewerBaseline.pages());
  expect(before?.map((p) => p.pon)).toEqual([LINKS, 6, FORM]);

  const bytes = await save(page);
  const { page: fresh, context } = await reopen(browser, info, bytes);
  try {
    const after = await fresh.evaluate(() => window.viewerBaseline.pages());
    // The object numbers ARE the identity: a client keeps per-page state
    // keyed on them, and a save that renumbered pages would silently move
    // every annotation, every bookmark and every cached raster one page
    // over.
    expect(after?.map((p) => p.pon)).toEqual(before?.map((p) => p.pon));
    expect(after?.map((p) => p.index)).toEqual(before?.map((p) => p.index));
    // And the pages are the same shape, so nothing was rotated on the way.
    expect(after?.map((p) => [p.width, p.height])).toEqual(
      before?.map((p) => [p.width, p.height]),
    );
  } finally {
    await context.close();
  }
});

test('F14 the viewer has no history to undo', async ({ page }, info) => {
  // F14 wants undo to restore the pre-mutation state and redo to restore the
  // post-mutation one. Neither can be written, because there is no history:
  // `packages/viewer/chrome/src/config/commands.ts` defines `history:undo`
  // and `history:redo` with an empty `run` and `enabled: () => false`, above
  // the comment "no history plugin in v3 yet". No plugin in the repository
  // provides one.
  //
  // So this pins what IS true. The buttons exist, a user can see them, and
  // they do nothing. The day a history plugin lands this assertion fails,
  // which is the signal to write the flow the port order actually asks for.
  await arrive(page, info, 'features');
  // Drawing first, because the Undo and Redo buttons live in the secondary
  // band that a tool mode opens, and because a history that existed would
  // have something in it by the time they are read.
  await drawRectangle(page, 0, { x: 420, y: 330 }, { x: 560, y: 230 });
  await expect.poll(async () => (await squares(page, LINKS)).length).toBe(1);

  for (const name of ['Undo', 'Redo']) {
    const button = page.getByRole('button', { name, exact: true }).first();
    await expect(button, `${name} is not shown`).toBeVisible();
    await expect(button, `${name} is enabled: a history plugin may have landed`).toBeDisabled();
  }
});
