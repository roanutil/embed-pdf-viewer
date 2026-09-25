import { test, expect, type Page } from '@playwright/test';
import { fileURLToPath } from 'node:url';
import type {} from '../../../examples/viewer-react/src/e2e';

/**
 * F9: the open failure path.
 *
 * A wrong password has to surface as a STATE the viewer can show, not as a
 * broken document. The tab comes up `locked`, a wrong attempt leaves it
 * locked and retryable, and the right one takes it to `ready`.
 *
 * What is NOT asserted is message text. The plan makes one error vocabulary
 * a requirement across both engines and says nothing about wording, so this
 * compares `EngineErrorCode` and leaves the prose to each implementation.
 *
 * `encrypted-r4-aes-128.pdf` comes from the engine conformance corpus rather
 * than from this package's own fixtures, because it is already generated,
 * already checked for reproducibility, and its user password is stated in
 * `tooling/engine-conformance/fixtures/generate.mjs`.
 */
const fixture = fileURLToPath(
  new URL('../../engine-conformance/fixtures/encrypted-r4-aes-128.pdf', import.meta.url),
);
const PASSWORD = 'user';
const errors = new WeakMap<Page, string[]>();

// Polled rather than read, everywhere. The bridge is installed by the
// viewer's `onReady`, and a failed unlock re-runs it: a bare `evaluate`
// between the two sees `window.viewerBaseline` undefined and reports a
// missing bridge instead of the status it was asked for.
const statuses = (page: Page) =>
  page.evaluate(() => window.viewerBaseline?.documentStatuses().map((d) => d.status) ?? null);

test.beforeEach(async ({ page, context }, info) => {
  errors.set(page, []);
  page.on('pageerror', (error) => errors.get(page)!.push(error.message));
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    return url.origin.startsWith('http://127.0.0.1:') || url.protocol === 'data:'
      ? route.continue()
      : route.abort('blockedbyclient');
  });
  await page.route('**/encrypted.pdf', (route) =>
    route.fulfill({ path: fixture, contentType: 'application/pdf' }),
  );
  await page.goto(`/e2e.html?backend=${info.project.metadata.backend}&doc=encrypted`);
});

test.afterEach(async ({ page }) => {
  expect(errors.get(page), 'uncaught browser errors').toEqual([]);
});

test('F9 a password-protected document comes up locked rather than failed', async ({ page }) => {
  // `locked` and `error` are different things to a viewer: one shows a
  // prompt, the other shows a dead tab. Reporting a password as an open
  // failure would send the user away from a document they can open.
  await expect.poll(() => statuses(page)).toEqual(['locked']);
});

test('F9 a wrong password leaves the document locked and retryable', async ({ page }) => {
  await expect.poll(() => statuses(page)).toEqual(['locked']);

  const refused = await page.evaluate(() => window.viewerBaseline.unlock('not-the-password'));
  expect(refused.ok).toBe(false);
  // The code, not the message. One vocabulary across both engines is the
  // contract; the wording is each implementation's own.
  expect(refused.code).toBe('DocPasswordIncorrect');

  // Nothing moved. The parked bytes are still there and the next attempt
  // does not have to transfer the document again.
  await expect.poll(() => statuses(page)).toEqual(['locked']);
});

test('F9 the right password opens the document', async ({ page }) => {
  await expect.poll(() => statuses(page)).toEqual(['locked']);

  const unlocked = await page.evaluate(
    (password) => window.viewerBaseline.unlock(password),
    PASSWORD,
  );
  expect(unlocked, 'the fixture password was refused').toMatchObject({ ok: true });

  await expect.poll(() => statuses(page)).toEqual(['ready']);
  // And the document is genuinely readable afterwards, not merely marked so.
  await expect
    .poll(() => page.evaluate(() => window.viewerBaseline.snapshot().pages.length))
    .toBeGreaterThan(0);
});

test('F9 a wrong password then the right one still opens', async ({ page }) => {
  // Retryable in the way a prompt needs: a user who mistypes once must not
  // have to reload. This is the flow that catches a session torn down on
  // the first refusal.
  await expect.poll(() => statuses(page)).toEqual(['locked']);
  await page.evaluate(() => window.viewerBaseline.unlock('wrong'));
  await expect.poll(() => statuses(page)).toEqual(['locked']);

  const unlocked = await page.evaluate(
    (password) => window.viewerBaseline.unlock(password),
    PASSWORD,
  );
  expect(unlocked).toMatchObject({ ok: true });
  await expect.poll(() => statuses(page)).toEqual(['ready']);
});
