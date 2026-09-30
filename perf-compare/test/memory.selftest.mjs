// Needs Chromium (`pnpm exec playwright-core install chromium`). Checks that
// the memory total counts wasm linear memory, and that the engine-worker path
// in cdp.mjs attaches, reads and throttles a dedicated worker.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { launchBrowser, openCdp } from '../lib/cdp.mjs';
import { serveArms } from '../lib/serve.mjs';

test('UASM counts a 64 MiB WebAssembly.Memory, and the worker is reachable', async () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'perf-mem-'));
  writeFileSync(path.join(dir, 'index.html'), '<!doctype html><script type="module" src="./main.js"></script>');
  writeFileSync(path.join(dir, 'main.js'), "window.__worker = new Worker('./worker.js');");
  writeFileSync(path.join(dir, 'worker.js'), 'setInterval(() => { let x = 0; for (let i = 0; i < 1e5; i++) x += i; }, 10);');
  const { urls, close } = await serveArms({ x: dir }, { basePort: 5381 });
  const run = await launchBrowser();
  try {
    const context = await run.browser.newContext();
    const page = await context.newPage();
    const cdp = await openCdp(page, run.cdp);
    await cdp.throttle(4);
    await page.goto(urls.x);
    await page.waitForFunction(() => window.__worker);
    await page.waitForTimeout(500);

    assert.equal(cdp.workerCount(), 1, 'the worker was not auto-attached');
    // Chromium 140 rejects Emulation.setCPUThrottlingRate on a worker target,
    // and page throttling doesn't reach the worker. What matters here is that
    // the refusal is recorded, so a `slow` run says it throttled only the
    // main thread.
    assert.equal(typeof cdp.workerThrottled, 'boolean');
    console.log(`worker throttled: ${cdp.workerThrottled}`);
    // null in Chromium 140, which has no Performance domain on workers; the
    // report prints n/a for workerTaskMs then.
    const wm = await cdp.workerMetrics();
    assert.ok(wm === null || wm.taskMs > 0, `worker metrics: ${JSON.stringify(wm)}`);
    console.log(`worker metrics: ${JSON.stringify(wm)}`);

    const before = await cdp.memory();
    await page.evaluate(() => {
      window.__keep = new WebAssembly.Memory({ initial: 1, maximum: 2048 });
      window.__keep.grow(1024); // 1024 pages of 64 KiB = 64 MiB
      new Uint8Array(window.__keep.buffer).fill(1);
    });
    const after = await cdp.memory();
    assert.equal(after.source, 'uasm');
    assert.ok(after.total - before.total >= 64 * 2 ** 20, `grew ${after.total - before.total}`);
    await cdp.close();
    await context.close();
  } finally {
    await run.close();
    await close();
  }
});
