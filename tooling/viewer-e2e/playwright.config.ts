import { defineConfig } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const backends = (process.env.VIEWER_E2E_BACKENDS ?? 'typescript').split(',');
for (const backend of backends) {
  if (backend !== 'typescript') throw new Error(`Unsupported viewer test backend: ${backend}`);
}
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
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
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
