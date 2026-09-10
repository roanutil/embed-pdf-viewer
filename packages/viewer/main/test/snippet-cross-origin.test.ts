/**
 * The snippet, loaded the way the docs show it: as a native ES module from
 * ANOTHER origin (jsDelivr in production; a local "CDN" here), into a plain
 * page. The regression this guards: every sibling the artifact needs — the
 * worker chunk, `embedpdf.wasm`, lazy chunks — must resolve relative to the
 * artifact's own folder and be fetched from that origin, and nowhere else.
 * A wasm URL computed against the wrong chunk, a CDN literal creeping into
 * the code, a missing CORS-safe sibling: all fail here, before they ship.
 *
 * Runs against the BUILT artifact (`pnpm build` first). Needs a Chromium:
 * Playwright's own (`npx playwright-core install chromium`, what CI does)
 * or a local Google Chrome as the fallback.
 */
import { createServer, type Server } from 'node:http';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { chromium, type Browser } from 'playwright-core';

const here = dirname(fileURLToPath(import.meta.url));
const dist = resolve(here, '..', 'dist');
const samplePdf = resolve(
  here,
  '..',
  '..',
  '..',
  '..',
  'examples',
  'engine-runtime-demo',
  'public',
  'sample.pdf',
);

const TYPES: Record<string, string> = {
  '.js': 'text/javascript',
  '.wasm': 'application/wasm',
  '.map': 'application/json',
  '.css': 'text/css',
  '.pdf': 'application/pdf',
  '.html': 'text/html',
};

/** A static server for one directory. `cors` = what a public CDN sends. */
function serveDirectory(root: string, cors: boolean, extra: Record<string, string> = {}): Server {
  return createServer((req, res) => {
    const pathname = decodeURIComponent((req.url ?? '/').split('?')[0]);
    const file = extra[pathname] ?? join(root, pathname);
    if (!file.startsWith(root) && !extra[pathname]) return void (res.writeHead(403), res.end());
    if (!existsSync(file) || statSync(file).isDirectory())
      return void (res.writeHead(404), res.end());
    res.writeHead(200, {
      'content-type': TYPES[extname(file)] ?? 'application/octet-stream',
      ...(cors ? { 'access-control-allow-origin': '*' } : {}),
    });
    res.end(readFileSync(file));
  });
}

const listen = (server: Server) =>
  new Promise<string>((ok) => {
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      ok(`http://127.0.0.1:${typeof address === 'object' && address ? address.port : 0}`);
    });
  });

// Two loopback ports stand in for two origins; Chrome's local-network-access
// check would deny that cross-origin fetch (it prompts a real user), which
// is a harness artifact, not a property of the artifact under test.
const LAUNCH = { args: ['--disable-features=LocalNetworkAccessChecks'] };

async function launch(): Promise<Browser> {
  try {
    return await chromium.launch(LAUNCH);
  } catch {
    return chromium.launch({ ...LAUNCH, channel: 'chrome' });
  }
}

describe('snippet from a foreign origin', () => {
  let cdn: Server;
  let site: Server;
  let cdnOrigin: string;
  let siteOrigin: string;
  let browser: Browser;

  beforeAll(async () => {
    expect(existsSync(join(dist, 'embedpdf.js')), 'run `pnpm build` first').toBe(true);
    cdn = serveDirectory(dist, true);
    cdnOrigin = await listen(cdn);
    // The customer's page: nothing of the artifact lives here — only the
    // (intercepted) HTML and the document it opens.
    site = serveDirectory(resolve(here, 'site'), false, { '/sample.pdf': samplePdf });
    siteOrigin = await listen(site);
    browser = await launch();
  });
  afterAll(async () => {
    await browser?.close();
    cdn?.close();
    site?.close();
  });

  it('renders, fetching every sibling from the artifact origin and nothing from anywhere else', async () => {
    const page = await browser.newPage();
    const requests: Array<{ url: string; status: number | null }> = [];
    const failures: string[] = [];
    const errors: string[] = [];
    page.on('response', (response) =>
      requests.push({ url: response.url(), status: response.status() }),
    );
    page.on('requestfailed', (request) =>
      failures.push(`${request.url()}: ${request.failure()?.errorText}`),
    );
    page.on('console', (message) => message.type() === 'error' && errors.push(message.text()));
    page.on('pageerror', (error) => errors.push(error.message));

    // The page the docs show, verbatim in shape: a container, one module import.
    const html = `<!doctype html><title>snippet</title>
<div id="pdf-viewer" style="height: 520px"></div>
<script type="module">
  import EmbedPDF from '${cdnOrigin}/embedpdf.js';
  EmbedPDF.init({ target: '#pdf-viewer', src: '/sample.pdf' });
</script>`;
    await page.route(`${siteOrigin}/`, (route) =>
      route.fulfill({ contentType: 'text/html', body: html }),
    );
    await page.goto(`${siteOrigin}/`);

    // Rendered = the chrome reports a page count for the opened document.
    try {
      await page.waitForFunction(
        () =>
          /Page 1 of \d+/.test(
            document.querySelector('embedpdf-viewer')?.shadowRoot?.textContent ?? '',
          ),
        null,
        { timeout: 60_000 },
      );
    } catch (error) {
      const shadowText = await page.evaluate(
        () =>
          document.querySelector('embedpdf-viewer')?.shadowRoot?.textContent?.slice(0, 400) ??
          '<no viewer element>',
      );
      throw new Error(
        `viewer did not render.\n shadow text: ${shadowText}\n requests: ${JSON.stringify(requests, null, 1)}\n failures: ${failures.join('; ')}\n console errors: ${errors.join('; ')}\n${String(error)}`,
      );
    }

    const wasm = requests.find((r) => r.url === `${cdnOrigin}/embedpdf.wasm`);
    expect(wasm?.status, 'embedpdf.wasm fetched from the artifact folder').toBe(200);
    const foreign = requests.filter(
      (r) => !r.url.startsWith(cdnOrigin) && !r.url.startsWith(siteOrigin),
    );
    expect(foreign, 'no request left the two origins').toEqual([]);
    expect(failures).toEqual([]);
    expect(errors.filter((e) => !/favicon/i.test(e))).toEqual([]);
    await page.close();
  });
});
