import { test, expect, type Page } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import type {} from '../../../examples/viewer-react/src/e2e';

/**
 * F6, F7 and F8: links, annotation display and form display.
 *
 * A separate fixture from the baseline flows, and a separate file. F1
 * through F5 read `baseline.pdf`'s pixels, its page count and its match
 * counts as their oracle; adding a link or a form field to it would move
 * every one of those for no reason.
 *
 * `features.pdf` is hand-authored in `fixtures/generate.mjs`, so the
 * expected values below are read out of the file's source rather than
 * computed by anything under test. Three pages: links, annotations, form.
 *
 * Every assertion reads a CAPABILITY rather than the DOM. The DOM is one
 * rendering of what the engine resolved, and asserting against it would pin
 * the layer's markup and its chrome settings alongside the thing being
 * tested. Where a flow has to prove the UI reacts, it drives a real click
 * and then reads the capability again.
 */
const fixture = fileURLToPath(new URL('../fixtures/features.pdf', import.meta.url));
const errors = new WeakMap<Page, string[]>();

/** The three pages, by the object numbers the file gives them. */
const LINKS = 5;
const ANNOTATIONS = 6;
const FORM = 7;

const snapshot = (page: Page) => page.evaluate(() => window.viewerBaseline.snapshot());
const stage = (page: Page) =>
  page.locator('[data-embedpdf-scrollbar][data-axis="y"]').locator('..');

/** Every image the viewer has painted has decoded. */
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
 * Scroll a page into view.
 *
 * Through the wheel rather than a page-number box, because the viewer's
 * chrome does not label one and a test should drive what a user drives.
 *
 * The fixture is three short pages, so the whole document is laid out at
 * once and this only has to bring the wanted one under the viewport's
 * reading line. The poll is on the page's own reported position rather than
 * on `currentPage`, because the last page of a short document can never
 * become current: there is nothing below it left to scroll.
 */
async function scrollPageIntoView(page: Page, index: number) {
  await stage(page).hover();
  const box = await stage(page).boundingBox();
  expect(box).not.toBeNull();
  for (let attempt = 0; attempt < 20; attempt++) {
    const rect = (await snapshot(page)).pages.find((r) => r.index === index);
    if (rect && rect.y >= 0 && rect.y + rect.height <= box!.height) break;
    await page.mouse.wheel(0, 400);
  }
  await painted(page);
}

/**
 * A point inside a page, from a PDF-space coordinate.
 *
 * The fixture's pages are 600x400 with the origin bottom-left, and the
 * viewer lays them out top-down, so the y flips. Computed from the stage's
 * own reported geometry rather than from a zoom level, because the viewer
 * picks the fit and the test should not have to predict it.
 */
async function pointAt(page: Page, pageIndex: number, pdfX: number, pdfY: number) {
  const box = await stage(page).boundingBox();
  expect(box).not.toBeNull();
  const state = await snapshot(page);
  const rect = state.pages.find((r) => r.index === pageIndex);
  expect(rect, `page ${pageIndex} is not visible`).toBeDefined();
  return {
    x: box!.x + rect!.x + (pdfX / 600) * rect!.width,
    y: box!.y + rect!.y + ((400 - pdfY) / 400) * rect!.height,
  };
}

test.beforeEach(async ({ page, context }, info) => {
  errors.set(page, []);
  page.on('pageerror', (error) => errors.get(page)!.push(error.message));
  // Nothing leaves the machine. The external link flow depends on this: it
  // asserts that activating a URI target does not navigate, and a real
  // request would make that assertion about the network.
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    return url.origin.startsWith('http://127.0.0.1:') || url.protocol === 'data:'
      ? route.continue()
      : route.abort('blockedbyclient');
  });
  await page.route('**/features.pdf', (route) =>
    route.fulfill({ path: fixture, contentType: 'application/pdf' }),
  );
  await page.goto(`/e2e.html?backend=${info.project.metadata.backend}&doc=features`);
  await expect(page.getByText('Page 1 of 3', { exact: true })).toBeVisible();
  await page.locator('embedpdf-viewer').evaluate((el) => {
    const style = document.createElement('style');
    style.textContent =
      '*, *::before, *::after { animation: none !important; transition: none !important; scroll-behavior: auto !important; }';
    el.shadowRoot!.append(style);
  });
  await painted(page);
});

test.afterEach(async ({ page }) => {
  expect(errors.get(page), 'uncaught browser errors').toEqual([]);
});

test('F6 resolves an internal link to its page and an external one to its URL', async ({
  page,
}) => {
  const links = await page.evaluate((pon) => window.viewerBaseline.links(pon), LINKS);
  expect(links, 'the link capability is not registered').not.toBeNull();

  const internal = links!.find((l) => l.target?.kind === 'goto');
  const external = links!.find((l) => l.target?.kind === 'uri');
  expect(internal, 'the fixture has a /GoTo link').toBeDefined();
  expect(external, 'the fixture has a /URI link').toBeDefined();

  // The URL exactly, because a link that resolves to almost the right place
  // is worse than one that fails: nothing reports it.
  expect(external!.target).toMatchObject({ uri: 'https://example.invalid/target' });
  // And the destination's page, which is page three in the file.
  expect(internal!.target).toMatchObject({
    kind: 'goto',
    destination: { pageObjectNumber: FORM },
  });
});

test('F6 lands the viewport on the target page when the link is clicked', async ({ page }) => {
  // Through a real click rather than by calling `activate`: what is being
  // tested is that the layer puts an anchor where the engine said the rect
  // is, and that activating it moves the camera.
  // The precondition, stated as a position rather than as a page number:
  // the target has to start somewhere the click can move it FROM, or the
  // assertion below would pass on a viewer that did nothing.
  const before = (await snapshot(page)).pages.find((r) => r.index === 2)!.y;
  expect(before, 'the target page starts below the fold').toBeGreaterThan(0);

  // The middle of the internal link's rect, which is [40 300 320 330].
  const at = await pointAt(page, 0, 180, 315);
  await page.mouse.click(at.x, at.y);

  // The target page comes fully into view, having started out of it.
  //
  // Not "reaches the top": the destination asks for the page's top edge and
  // this document is three short pages, so the viewer runs out of scroll
  // before it gets there. Landing the page in the viewport is what the
  // reader asked for and what holds whatever the document's length.
  //
  // Not `currentPage` either, which for the last page of a short document
  // never changes: there is nothing below it left to scroll.
  const box = await stage(page).boundingBox();
  await expect
    .poll(async () => {
      const rect = (await snapshot(page)).pages.find((r) => r.index === 2);
      return rect !== undefined && rect.y >= 0 && rect.y + rect.height <= box!.height;
    })
    .toBe(true);
  expect((await snapshot(page)).pages.find((r) => r.index === 2)!.y).toBeLessThan(before);
  await painted(page);
});

test('F6 does not navigate when an external link is activated', async ({ page }) => {
  // A URI target is the framework layer's to open, and opening it must not
  // take the viewer with it. A regression here loses the user's document.
  const before = page.url();
  const at = await pointAt(page, 0, 200, 255);
  await page.mouse.click(at.x, at.y);

  // Given a moment to do the wrong thing, then asserted. A poll would pass
  // the instant it looked, before a navigation had started.
  await page.waitForTimeout(250);
  expect(page.url()).toBe(before);
  expect((await snapshot(page)).documents).toEqual(['ready']);
});

test('F7 paints the fixture annotations at the rects the file gives them', async ({ page }) => {
  await scrollPageIntoView(page, 1);

  const annotations = await page.evaluate(
    (pon) => window.viewerBaseline.annotations(pon),
    ANNOTATIONS,
  );
  expect(annotations, 'the annotation capability is not registered').not.toBeNull();

  const byName = new Map(annotations!.map((a) => [a.nm, a]));
  expect([...byName.keys()].sort()).toEqual(['annot-highlight', 'annot-square']);
  expect(byName.get('annot-square')).toMatchObject({
    subtype: 'square',
    // PDF space, y-up, exactly as written into the fixture.
    rect: { left: 60, bottom: 240, right: 260, top: 340 },
  });
  expect(byName.get('annot-highlight')).toMatchObject({
    subtype: 'highlight',
    rect: { left: 40, bottom: 140, right: 400, top: 180 },
  });
});

test('F7 selects the annotation under the pointer and not its neighbour', async ({ page }) => {
  await scrollPageIntoView(page, 1);

  // Empty page first, so the assertion below cannot pass on a layer that
  // reports a selection whatever the pointer is over.
  const empty = await pointAt(page, 1, 500, 60);
  await page.mouse.click(empty.x, empty.y);
  await expect
    .poll(() => page.evaluate(() => window.viewerBaseline.annotationSelection()?.length ?? 0))
    .toBe(0);

  // Then the square's interior, well clear of the highlight below it.
  const at = await pointAt(page, 1, 160, 290);
  await page.mouse.click(at.x, at.y);
  await expect
    .poll(() => page.evaluate(() => window.viewerBaseline.annotationSelection()?.length ?? 0))
    .toBe(1);
});

test('F8 reports every field value the file states', async ({ page }) => {
  await scrollPageIntoView(page, 2);

  const fields = await page.evaluate(() => window.viewerBaseline.formFields());
  expect(fields, 'the form capability is not registered').not.toBeNull();

  const byName = new Map(fields!.map((f) => [f.name, f]));
  expect([...byName.keys()].sort()).toEqual(['fullName', 'language', 'subscribe']);

  // A text field's string, a checkbox's state and a list box's selection:
  // three families, three shapes, all read from one snapshot.
  expect(byName.get('fullName')).toMatchObject({ family: 'text', value: 'Ada Lovelace' });
  expect(byName.get('subscribe')).toMatchObject({ family: 'checkbox', checked: true });
  expect(byName.get('language')).toMatchObject({ family: 'listbox', selectedValues: ['fr'] });
});
