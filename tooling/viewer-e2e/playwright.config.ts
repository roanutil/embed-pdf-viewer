import { defineConfig } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
/**
 * Which engine backend each project drives.
 *
 * `typescript` is the shipping one and the default. `rust` routes the same
 * viewer through the Rust engine core, which only exists in a runtime payload
 * built with it: `pnpm --filter @embedpdf/engine-runtime build:target wasm32`
 * links it, and a payload built with `EMBEDPDF_RUST_CORE=0` does not. On a
 * payload without it the viewer falls back and the run proves nothing, so the
 * Rust projects assert the core is present before they start.
 *
 * The flows are written ONCE and run under both. That is the whole point of
 * the plan's differential rule: two implementations of one contract are only
 * worth having if the same assertions run against each.
 */
const backends = (process.env.VIEWER_E2E_BACKENDS ?? 'typescript').split(',');
for (const backend of backends) {
  if (backend !== 'typescript' && backend !== 'rust') {
    throw new Error(`Unsupported viewer test backend: ${backend}`);
  }
}
const performanceEnabled = process.env.VIEWER_E2E_PERF === '1';
const packed = process.env.VIEWER_E2E_PACKED_DIR;
const port = packed ? 5241 : 5240;
export default defineConfig({
  testDir: './tests',
  outputDir: packed ? './test-results/packed' : './test-results/workspace',
  timeout: 45_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: [
    ...(performanceEnabled ? [['./performance-reporter.ts'] as [string]] : []),
    ['list'],
    [
      'html',
      {
        open: 'never',
        outputFolder: packed ? 'playwright-report/packed' : 'playwright-report/workspace',
      },
    ],
  ],
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    viewport: { width: 1280, height: 900 },
    deviceScaleFactor: 1,
    locale: 'en-US',
    timezoneId: 'UTC',
    colorScheme: 'light',
    reducedMotion: 'reduce',
    serviceWorkers: 'block',
    trace: performanceEnabled ? 'off' : 'retain-on-failure',
    screenshot: performanceEnabled ? 'off' : 'only-on-failure',
  },
  projects: backends.flatMap((backend) =>
    (['chromium', 'firefox', 'webkit'] as const).map((browserName) => ({
      name: `${browserName}-${backend}`,
      metadata: { backend },
      use: { browserName },
    })),
  ),
  webServer: {
    command: packed
      ? `npm exec vite -- preview --host 127.0.0.1 --port ${port} --strictPort`
      : 'pnpm --filter @embedpdf/example-viewer-react dev --host 127.0.0.1',
    cwd: packed ?? root,
    url: `http://127.0.0.1:${port}/e2e.html`,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
