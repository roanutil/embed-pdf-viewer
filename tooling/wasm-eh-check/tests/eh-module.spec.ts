import { expect, test } from '@playwright/test';

interface Report {
  ok: boolean;
  error: string | null;
  steps: { name: string; value: unknown }[];
}

/**
 * The module loads and does real PDFium work, including both paths the EH
 * build exists to carry: C++ exceptions, and the setjmp/longjmp that libpng
 * and libjpeg use for error handling.
 */
/**
 * Which build actually ran. The distinction this whole harness exists for is
 * release browser versus engine build, and "firefox-release passed" is only
 * worth anything next to the version string it passed in. Printed rather than
 * attached, because the `list` reporter shows stdout and swallows annotations.
 */
test('records the browser it ran in', async ({ browser, browserName }, info) => {
  console.log(`${info.project.name}: ${browserName} ${browser.version()}`);
});

test('the wasm32-eh module runs', async ({ page }, info) => {
  const consoleErrors: string[] = [];
  page.on('pageerror', (error) => consoleErrors.push(error.message));
  page.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(message.text());
  });

  await page.goto('/');
  await expect
    .poll(() => page.evaluate(() => Boolean(window.__ehResult)), {
      message: 'the page never finished; the module probably failed to instantiate',
    })
    .toBe(true);

  const report = (await page.evaluate(() => window.__ehResult)) as Report;
  const steps = Object.fromEntries(report.steps.map((s) => [s.name, s.value]));
  await info.attach('eh-report', {
    contentType: 'application/json',
    body: Buffer.from(JSON.stringify({ project: info.project.name, ...report }, null, 2)),
  });

  expect(report.error, 'the page threw').toBeNull();
  expect(report.ok).toBe(true);

  expect(steps.instantiated).toBe(true);
  expect(steps.documentOpened).toBe(true);
  expect(steps.pageCount).toBe(14);
  expect(steps.charCount).toBeGreaterThan(1000);

  // The raster path, which reaches the setjmp/longjmp this build lowers
  // through wasm EH.
  expect(steps.renderedNonWhitePixels as number).toBeGreaterThan(0);

  // The C++ exception path: PDFium throws and catches internally on a wrong
  // password. 4 is FPDF_ERR_PASSWORD.
  expect(steps.wrongPasswordRefused).toBe(true);
  expect(steps.lastError).toBe(4);

  expect(consoleErrors, 'uncaught browser errors').toEqual([]);
});

test('the linked Rust core runs in the browser', async ({ page }) => {
  await page.goto('/');
  await expect.poll(() => page.evaluate(() => Boolean(window.__ehResult))).toBe(true);
  const report = (await page.evaluate(() => window.__ehResult)) as Report;
  const steps = Object.fromEntries(report.steps.map((s) => [s.name, s.value]));

  test.skip(steps.rustCoreLinked !== true, 'this module was built without the Rust core');
  // The same page of the same document the Rust corpus vectors describe, so a
  // browser that ran the Rust reader differently would show here.
  expect(steps.rustGeometryRuns).toBe(238);
  expect(steps.rustGeometryGlyphs).toBe(5087);
});

declare global {
  interface Window {
    __ehResult?: unknown;
  }
}
