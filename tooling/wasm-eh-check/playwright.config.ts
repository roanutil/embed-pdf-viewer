import { defineConfig, devices } from '@playwright/test';

/**
 * Phase 1's exit gate asks for the "real linked EH module tested in release
 * Chrome, Firefox, Safari". Section 14 is blunter about why: the historical
 * audit's only recorded passes were Chrome and a Playwright Firefox NIGHTLY,
 * Safari was untested, and there is no fallback if a browser we support
 * refuses the module.
 *
 * So the projects are split by what they actually prove. `chrome` is the real
 * release browser installed on this machine and is evidence for the floor.
 * The three engine projects are Playwright's own builds: they catch a module
 * that no engine will load, which is worth having, but a Playwright Firefox
 * pass is not a release Firefox pass and the plan says so in as many words.
 */
const port = Number(process.env.EH_PORT ?? 5311);

export default defineConfig({
  testDir: './tests',
  timeout: 120_000,
  expect: { timeout: 60_000 },
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: [['list']],
  use: { baseURL: `http://127.0.0.1:${port}` },
  projects: [
    // Release browsers. These are the ones a floor can be announced from.
    { name: 'chrome-release', use: { ...devices['Desktop Chrome'], channel: 'chrome' } },
    // `moz-firefox` drives the Firefox in /Applications over WebDriver BiDi,
    // rather than Playwright's patched build. That is the whole point: the
    // plan's existing Firefox evidence was a Playwright Nightly.
    { name: 'firefox-release', use: { ...devices['Desktop Firefox'], channel: 'moz-firefox' } },
    // Engine builds. Useful, and not the same claim.
    { name: 'playwright-chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'playwright-firefox', use: { ...devices['Desktop Firefox'] } },
    { name: 'playwright-webkit', use: { ...devices['Desktop Safari'] } },
  ],
  webServer: {
    command: 'node serve.mjs',
    url: `http://127.0.0.1:${port}/`,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
